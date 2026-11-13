use std::sync::Arc;

use axum::{
    Json,
    extract::{
        Path,
        State,
    },
    http::StatusCode,
};

use crate::{
    article::{
        Article,
        NewArticle,
    },
    repository::{
        ArticleRepository,
        RepositoryError,
    },
};

#[tracing::instrument(
    level = "INFO",
    skip(repository, article),
    fields(continent = %continent)
)]
pub async fn create_article(
    State(repository): State<Arc<ArticleRepository>>,
    Path(continent): Path<String>,
    Json(article): Json<NewArticle>,
) -> Result<(StatusCode, Json<Article>), StatusCode> {
    let article = repository
        .create(&continent, &article)
        .await
        .map_err(status_code)?;

    Ok((StatusCode::CREATED, Json(article)))
}

#[tracing::instrument(
    level = "INFO",
    skip(repository),
    fields(continent = %continent, id = %id, cache = tracing::field::Empty)
)]
pub async fn get_article(
    State(repository): State<Arc<ArticleRepository>>,
    Path((continent, id)): Path<(String, i64)>,
) -> Result<Json<Article>, StatusCode> {
    match repository.get(&continent, id).await {
        Ok(Some(article)) => Ok(Json(article)),
        Ok(None) => {
            tracing::debug!(continent = %continent, id = %id, "article not found");
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => Err(status_code(e)),
    }
}

#[tracing::instrument(
    level = "INFO",
    skip(repository, article),
    fields(continent = %continent, id = %id)
)]
pub async fn update_article(
    State(repository): State<Arc<ArticleRepository>>,
    Path((continent, id)): Path<(String, i64)>,
    Json(article): Json<NewArticle>,
) -> Result<Json<Article>, StatusCode> {
    match repository.update(&continent, id, &article).await {
        Ok(Some(article)) => Ok(Json(article)),
        Ok(None) => {
            tracing::debug!(continent = %continent, id = %id, "article not found");
            Err(StatusCode::NOT_FOUND)
        }
        Err(e) => Err(status_code(e)),
    }
}

#[tracing::instrument(
    level = "INFO",
    skip(repository),
    fields(continent = %continent, id = %id)
)]
pub async fn delete_article(
    State(repository): State<Arc<ArticleRepository>>,
    Path((continent, id)): Path<(String, i64)>,
) -> Result<StatusCode, StatusCode> {
    let deleted = repository
        .delete(&continent, id)
        .await
        .map_err(status_code)?;

    if !deleted {
        tracing::debug!(continent = %continent, id = %id, "article not found");
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

fn status_code(error: RepositoryError) -> StatusCode {
    match error {
        RepositoryError::UnknownContinent => StatusCode::BAD_REQUEST,
        RepositoryError::Database => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
