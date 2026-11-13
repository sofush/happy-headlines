use std::time::Duration;

use anyhow::anyhow;
use redis::{
    AsyncCommands as _,
    aio::MultiplexedConnection,
};
use tracing::Instrument as _;

use crate::article::Article;

pub const CACHE_PURGE_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

pub struct ArticleCache {
    redis: MultiplexedConnection,
}

impl ArticleCache {
    pub fn new(redis: MultiplexedConnection) -> Self {
        Self { redis }
    }

    pub async fn get(
        &self,
        continent: &str,
        id: i64,
    ) -> anyhow::Result<Article> {
        let mut redis = self.redis.clone();

        async {
            let value: String = redis.get(cache_key(continent, id)).await?;
            serde_json::from_str::<_>(&value).map_err(|e| anyhow!(e))
        }
        .instrument(tracing::debug_span!(
            "cache.get",
            "db.system" = "redis",
            "db.operation.name" = "get",
            continent,
            "article.id" = %id
        ))
        .await
    }

    pub async fn store(&self, article: &Article) -> anyhow::Result<()> {
        let mut redis = self.redis.clone();

        async {
            let value = serde_json::to_string(article)?;
            redis
                .set_ex::<_, _, ()>(
                    cache_key(&article.continent, article.id),
                    value,
                    CACHE_PURGE_AGE.as_secs(),
                )
                .await?;
            Ok(())
        }
        .instrument(tracing::debug_span!(
            "cache.set",
            "db.system" = "redis",
            "db.operation.name" = "set",
            continent = %article.continent,
            "article.id" = %article.id
        ))
        .await
    }

    pub async fn evict(&self, continent: &str, id: i64) -> anyhow::Result<()> {
        let mut redis = self.redis.clone();

        async {
            redis.del::<_, ()>(cache_key(continent, id)).await?;
            Ok(())
        }
        .instrument(tracing::debug_span!(
            "cache.del",
            "db.system" = "redis",
            "db.operation.name" = "del",
            continent,
            "article.id" = %id
        ))
        .await
    }
}

fn cache_key(continent: &str, id: i64) -> String {
    format!("articles:{continent}:{id}")
}
