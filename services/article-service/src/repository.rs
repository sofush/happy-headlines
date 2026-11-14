use std::{
    collections::HashMap,
    sync::LazyLock,
};

use metrics::Counter;
use tokio_postgres::{
    Client,
    Error as PostgresError,
};

use crate::{
    article::{
        Article,
        NewArticle,
    },
    cache::ArticleCache,
};

static ARTICLE_CACHE_HITS_TOTAL: LazyLock<Counter> =
    LazyLock::new(|| metrics::counter!("article_cache_hits_total"));

static ARTICLE_CACHE_MISSES_TOTAL: LazyLock<Counter> =
    LazyLock::new(|| metrics::counter!("article_cache_misses_total"));

#[derive(Debug)]
pub enum RepositoryError {
    UnknownContinent,
    Database,
}

#[derive(Debug)]
pub struct ArticleRepository {
    database: ArticleDatabase,
    cache: ArticleCache,
}

impl ArticleRepository {
    pub fn new(shards: HashMap<String, Client>, cache: ArticleCache) -> Self {
        Self {
            database: ArticleDatabase { shards },
            cache,
        }
    }

    pub async fn create(
        &self,
        continent: &str,
        article: &NewArticle,
    ) -> Result<Article, RepositoryError> {
        let article = self.database.create(continent, article).await?;
        self.cache_article(&article).await;

        Ok(article)
    }

    #[tracing::instrument(
        level = "INFO",
        fields(continent = %continent, id = %id)
    )]
    pub async fn get(
        &self,
        continent: &str,
        id: i64,
    ) -> Result<Option<Article>, RepositoryError> {
        tracing::info!("Entered ArticleRepository::get");

        if let Ok(article) = self.cache.get(continent, id).await {
            tracing::Span::current().record("cache", "hit");
            ARTICLE_CACHE_HITS_TOTAL.increment(1);
            return Ok(Some(article));
        }

        tracing::Span::current().record("cache", "miss");
        ARTICLE_CACHE_MISSES_TOTAL.increment(1);

        let article = self.database.get(continent, id).await?;

        if let Some(article) = &article {
            self.cache_article(article).await;
        }

        Ok(article)
    }

    pub async fn update(
        &self,
        continent: &str,
        id: i64,
        article: &NewArticle,
    ) -> Result<Option<Article>, RepositoryError> {
        let updated = self.database.update(continent, id, article).await?;

        if let Some(article) = &updated {
            self.cache_article(article).await;
        }

        Ok(updated)
    }

    pub async fn delete(
        &self,
        continent: &str,
        id: i64,
    ) -> Result<bool, RepositoryError> {
        let deleted = self.database.delete(continent, id).await?;

        if let Err(e) = self.cache.evict(continent, id).await {
            tracing::warn!(
                continent = %continent,
                id = %id,
                error = %e,
                "could not evict deleted article"
            );
        }

        Ok(deleted)
    }

    #[tracing::instrument(
        level = "INFO",
        fields(id = %article.id, continent = %article.continent, title = %article.title, published_at = %article.published_at)
    )]
    async fn cache_article(&self, article: &Article) {
        if let Err(e) = self.cache.store(article).await {
            tracing::warn!(
                continent = %article.continent,
                article.id = article.id,
                error = %e,
                "could not cache article"
            );
        }
    }
}

#[derive(Debug)]
struct ArticleDatabase {
    shards: HashMap<String, Client>,
}

impl ArticleDatabase {
    fn shard(&self, continent: &str) -> Result<&Client, RepositoryError> {
        self.shards.get(continent).ok_or_else(|| {
            tracing::warn!(continent, "rejected request for unknown continent");
            RepositoryError::UnknownContinent
        })
    }

    async fn create(
        &self,
        continent: &str,
        article: &NewArticle,
    ) -> Result<Article, RepositoryError> {
        let shard = self.shard(continent)?;

        let row = shard
            .query_one(
                "INSERT INTO articles (continent, title, content) VALUES ($1, $2, $3) RETURNING id, continent, title, content, published_at",
                &[&continent, &article.title, &article.content],
            )
            .await
            .map_err(|e| database_error(continent, "create", e))?;

        Ok(Article::from_row(row))
    }

    #[tracing::instrument(
        level = "INFO",
        fields(continent = %continent, id = %id)
    )]
    async fn get(
        &self,
        continent: &str,
        id: i64,
    ) -> Result<Option<Article>, RepositoryError> {
        tracing::info!(
            "Fetching article {id} from SQL database (continent {continent})"
        );
        let shard = self.shard(continent)?;

        let row = shard
            .query_opt(
                "SELECT id, continent, title, content, published_at FROM articles WHERE id = $1",
                &[&id],
            )
            .await
            .map_err(|e| database_error(continent, "read", e))?;

        Ok(row.map(Article::from_row))
    }

    async fn update(
        &self,
        continent: &str,
        id: i64,
        article: &NewArticle,
    ) -> Result<Option<Article>, RepositoryError> {
        let shard = self.shard(continent)?;

        let row = shard
            .query_opt(
                "UPDATE articles SET title = $2, content = $3 WHERE id = $1 RETURNING id, continent, title, content, published_at",
                &[&id, &article.title, &article.content],
            )
            .await
            .map_err(|e| database_error(continent, "update", e))?;

        Ok(row.map(Article::from_row))
    }

    async fn delete(
        &self,
        continent: &str,
        id: i64,
    ) -> Result<bool, RepositoryError> {
        let shard = self.shard(continent)?;

        let deleted = shard
            .execute("DELETE FROM articles WHERE id = $1", &[&id])
            .await
            .map_err(|e| database_error(continent, "delete", e))?;

        Ok(deleted > 0)
    }
}

fn database_error(
    continent: &str,
    action: &str,
    error: PostgresError,
) -> RepositoryError {
    tracing::error!(continent, action, error = %error, "database query failed");
    RepositoryError::Database
}
