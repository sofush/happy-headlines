use std::time::Duration;

use chrono::{
    DateTime,
    Utc,
};
use redis::AsyncCommands as _;

use crate::{
    AppState,
    article::Article,
    cache,
};

use anyhow::anyhow;

pub fn spawn(state: std::sync::Arc<AppState>) {
    let fill_interval = Duration::from_secs(30 * 60);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(fill_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;

            if let Err(e) = refresh(&state).await {
                eprintln!("{} cache refresh failed: {e}", state.instance);
            }
        }
    });
}

async fn refresh(state: &AppState) -> anyhow::Result<()> {
    if !cache::acquire_filler_lease(state.redis.clone(), &state.instance)
        .await?
    {
        eprintln!(
            "{} skipping cache refresh, another replica holds the lease",
            state.instance
        );
        return Ok(());
    }

    let date_cutoff: DateTime<Utc> =
        Utc::now() - Duration::from_secs(14 * 24 * 60 * 60);

    let filled = fill_with_latest_articles(state, date_cutoff).await?;
    let evicted = evict_old_articles(state, date_cutoff).await?;

    println!(
        "{} cached {filled} recent articles and evicted {evicted} expired ones",
        state.instance
    );

    Ok(())
}

async fn fill_with_latest_articles(
    state: &AppState,
    cutoff: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let mut articles = Vec::new();

    for (continent, shard) in &state.shards {
        let rows = shard
            .query(
                r#"
                SELECT id, continent, title, content, published_at
                FROM articles
                WHERE published_at >= $1
                "#,
                &[&cutoff],
            )
            .await
            .map_err(|e| anyhow!("{continent}: {e}"))?;

        for row in rows {
            articles.push(Article::from_row(row));
        }
    }

    let mut redis = state.redis.clone();
    let mut pipe = redis::pipe();

    for a in &articles {
        pipe.set(
            cache::cache_key(&a.continent, a.id),
            serde_json::to_string(a)?,
        )
        .ignore();
    }

    pipe.query_async::<()>(&mut redis).await?;
    Ok(articles.len())
}

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
