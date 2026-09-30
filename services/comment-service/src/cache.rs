use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use redis::AsyncCommands as _;

use crate::comment::{
    Comment,
    CommentQuery,
};

use anyhow::anyhow;

async fn touch_lru(
    mut redis: redis::aio::MultiplexedConnection,
    key: &str,
) -> anyhow::Result<()> {
    let access_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    redis
        .zadd::<_, _, _, ()>("comments:lru", &key, access_time)
        .await?;

    let count: usize = redis.zcard("comments:lru").await?;

    if count > 30 {
        let remove_count = count - 30;

        let oldest: Vec<String> = redis
            .zrange("comments:lru", 0, remove_count as isize - 1)
            .await?;

        for key in oldest {
            redis::pipe()
                .del(&key)
                .zrem("comments:lru", &key)
                .query_async::<()>(&mut redis)
                .await?;
        }
    }

    Ok(())
}

pub async fn get_cached_comments(
    mut redis: redis::aio::MultiplexedConnection,
    query: &CommentQuery,
) -> anyhow::Result<Vec<Comment>> {
    let key = format!("comments:{}:{}", query.continent, query.article_id);
    let value: String = redis.get(&key).await?;
    touch_lru(redis.clone(), &key).await?;
    serde_json::from_str::<_>(&value).map_err(|e| anyhow!(e))
}

pub async fn store_cached_comments(
    mut redis: redis::aio::MultiplexedConnection,
    query: &CommentQuery,
    comments: Vec<Comment>,
) -> anyhow::Result<()> {
    let key = format!("comments:{}:{}", query.continent, query.article_id);
    let value = serde_json::to_string(&comments)?;
    redis.set::<_, _, ()>(&key, value).await?;
    touch_lru(redis.clone(), &key).await.map_err(|e| anyhow!(e))
}
