use std::time::Duration;

use chrono::{
    DateTime,
    Utc,
};
use redis::AsyncCommands as _;
use tracing::Instrument as _;

use crate::{
    AppState,
    article::Article,
    cache,
};

use anyhow::anyhow;

pub fn spawn(state: std::sync::Arc<AppState>) {
    let fill_interval = Duration::from_secs(30 * 60);
    let instance = state.instance.clone();

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(fill_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            let refresh = async {
                if let Err(e) = refresh(&state).await {
                    tracing::error!(error = %format!("{e:#}"), "cache refresh failed");
                }
            };

            refresh
                .instrument(
                    tracing::info_span!("cache.refresh", instance = %instance),
                )
                .await;

            ticker.tick().await;
        }
    });
}

#[tracing::instrument(level = "INFO", skip(state))]
async fn refresh(state: &AppState) -> anyhow::Result<()> {
    if !cache::acquire_filler_lease(state.redis.clone(), &state.instance)
        .await?
    {
        tracing::debug!(
            "skipping cache refresh, another replica holds the lease"
        );
        return Ok(());
    }

    let date_cutoff: DateTime<Utc> = Utc::now() - cache::CACHE_PURGE_AGE;
    let filled = fill_with_latest_articles(state, date_cutoff).await?;
    let evicted = evict_old_articles(state, date_cutoff).await?;

    tracing::info!(filled, evicted, "refreshed the article cache");
    Ok(())
}

#[tracing::instrument(level = "DEBUG", skip(state, cutoff))]
async fn fill_with_latest_articles(
    state: &AppState,
    cutoff: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let mut articles = Vec::new();

    for (continent, shard) in &state.shards {
        let rows = async {
            shard
                .query(
                    r#"
                    SELECT id, continent, title, content, published_at
                    FROM articles
                    WHERE published_at >= $1
                    "#,
                    &[&cutoff],
                )
                .await
                .map_err(|e| anyhow!("{continent}: {e}"))
        }
        .instrument(tracing::debug_span!(
            "db.query",
            "db.system" = "postgresql",
            "db.operation.name" = "select_recent_articles",
            continent = %continent
        ))
        .await?;

        for row in rows {
            articles.push(Article::from_row(row));
        }
    }

    if articles.is_empty() {
        return Ok(0);
    }

    let mut redis = state.redis.clone();
    let mut pipe = redis::pipe();

    for a in &articles {
        pipe.set_ex(
            cache::cache_key(&a.continent, a.id),
            serde_json::to_string(a)?,
            cache::CACHE_PURGE_AGE.as_secs(),
        )
        .ignore();
    }

    async { pipe.query_async::<()>(&mut redis).await }
        .instrument(tracing::debug_span!(
            "cache.warm",
            "db.system" = "redis",
            articles = articles.len()
        ))
        .await?;

    Ok(articles.len())
}

#[tracing::instrument(level = "DEBUG", skip(state, cutoff))]
async fn evict_old_articles(
    state: &AppState,
    cutoff: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let keys: Vec<String> = {
        let mut scan = state.redis.clone();
        let mut cursor = scan.scan_match::<_, String>("articles:*").await?;
        let mut keys = Vec::new();

        while let Some(key) = cursor.next_item().await {
            keys.push(key?);
        }

        keys
    };

    if keys.is_empty() {
        return Ok(0);
    }

    let mut redis = state.redis.clone();

    let values: Vec<Option<String>> = redis.mget(&keys).await?;
    let expired: Vec<String> = keys
        .iter()
        .zip(&values)
        .filter_map(|(key, value)| {
            let value = value.as_ref()?;
            let article = serde_json::from_str::<Article>(value).ok()?;
            (article.published_at < cutoff).then(|| key.clone())
        })
        .collect();

    if expired.is_empty() {
        return Ok(0);
    }

    redis
        .del::<_, usize>(&expired)
        .await
        .map_err(|e| anyhow!(e))
}
