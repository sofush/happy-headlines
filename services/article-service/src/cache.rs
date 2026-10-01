use std::time::Duration;

use redis::{
    AsyncCommands as _,
    ExistenceCheck,
    SetExpiry,
    SetOptions,
};

use crate::article::Article;

use anyhow::anyhow;

const FILLER_LEASE_KEY: &str = "articlecache:filler";
const FILLER_LEASE_TTL: Duration = Duration::from_secs(30 * 60);

pub fn cache_key(continent: &str, id: i64) -> String {
    format!("articles:{continent}:{id}")
}

pub async fn get_cached_article(
    mut redis: redis::aio::MultiplexedConnection,
    continent: &str,
    id: i64,
) -> anyhow::Result<Article> {
    let value: String = redis.get(cache_key(continent, id)).await?;
    serde_json::from_str::<_>(&value).map_err(|e| anyhow!(e))
}

pub async fn store_cached_article(
    mut redis: redis::aio::MultiplexedConnection,
    continent: &str,
    id: i64,
    article: &Article,
) -> anyhow::Result<()> {
    let value = serde_json::to_string(article)?;
    redis
        .set::<_, _, ()>(cache_key(continent, id), value)
        .await?;
    Ok(())
}

pub async fn evict_cached_article(
    mut redis: redis::aio::MultiplexedConnection,
    continent: &str,
    id: i64,
) -> anyhow::Result<()> {
    redis.del::<_, ()>(cache_key(continent, id)).await?;
    Ok(())
}

pub async fn acquire_filler_lease(
    mut redis: redis::aio::MultiplexedConnection,
    instance: &str,
) -> anyhow::Result<bool> {
    let claim: Option<String> = redis
        .set_options(
            FILLER_LEASE_KEY,
            instance,
            SetOptions::default()
                .conditional_set(ExistenceCheck::NX)
                .with_expiration(SetExpiry::EX(FILLER_LEASE_TTL.as_secs())),
        )
        .await?;

    Ok(claim.is_some())
}
