use redis::AsyncCommands as _;

use crate::comment::{
    Comment,
    CommentQuery,
};

use anyhow::anyhow;

pub async fn get_cached_comments(
    mut redis: redis::aio::MultiplexedConnection,
    query: &CommentQuery,
) -> anyhow::Result<Vec<Comment>> {
    let key = format!("comments:{}:{}", query.continent, query.article_id);
    let value: String = redis.get(key).await?;
    serde_json::from_str::<_>(&value).map_err(|e| anyhow!(e))
}

pub async fn store_cached_comments(
    mut redis: redis::aio::MultiplexedConnection,
    query: &CommentQuery,
    comments: Vec<Comment>,
) -> anyhow::Result<()> {
    let key = format!("comments:{}:{}", query.article_id, query.continent);
    let value = serde_json::to_string(&comments)?;
    redis.set(key, value).await.map_err(|e| anyhow!(e))
}
