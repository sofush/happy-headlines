use std::{
    collections::HashMap,
    sync::Arc,
};

use axum::{
    Json,
    Router,
    extract::{
        Path,
        State,
    },
    http::StatusCode,
    routing::{
        get,
        post,
    },
};
use tokio_postgres::{
    Client,
    NoTls,
};

use crate::article::{
    Article,
    NewArticle,
};

const REDIS_URL: &str = "redis://redis-articles-cache:6379/";

const CONTINENTS: &[(&str, &str)] = &[
    ("africa", "articles_africa"),
    ("antarctica", "articles_antarctica"),
    ("asia", "articles_asia"),
    ("europe", "articles_europe"),
    ("north-america", "articles_north_america"),
    ("oceania", "articles_oceania"),
    ("south-america", "articles_south_america"),
    ("global", "articles_global"),
];

mod article;
mod cache;
mod filler;

pub struct AppState {
    pub shards: HashMap<String, Client>,
    pub instance: String,
    pub redis: redis::aio::MultiplexedConnection,
}

async fn create_article(
    State(state): State<Arc<AppState>>,
    Path(continent): Path<String>,
    Json(article): Json<NewArticle>,
) -> Result<(StatusCode, Json<Article>), StatusCode> {
    let shard = shard(&state, &continent)?;

    let row = shard
        .query_one(
            "INSERT INTO articles (continent, title, content) VALUES ($1, $2, $3) RETURNING id, continent, title, content, published_at",
            &[&continent, &article.title, &article.content],
        )
        .await
        .map_err(|e| database_error(&continent, "create", e))?;

    let article = Article::from_row(row);

    if let Err(e) = cache::store_cached_article(
        state.redis.clone(),
        &continent,
        article.id,
        &article,
    )
    .await
    {
        eprintln!(
            "{} could not cache created article {}: {e}",
            state.instance, article.id
        );
    }

    log(&state, "create", &continent);

    Ok((StatusCode::CREATED, Json(article)))
}

async fn get_article(
    State(state): State<Arc<AppState>>,
    Path((continent, id)): Path<(String, i64)>,
) -> Result<Json<Article>, StatusCode> {
    let shard = shard(&state, &continent)?;

    if let Ok(article) =
        cache::get_cached_article(state.redis.clone(), &continent, id).await
    {
        log(&state, "read", &continent);
        return Ok(Json(article));
    }

    let row = shard
        .query_opt(
            "SELECT id, continent, title, content, published_at FROM articles WHERE id = $1",
            &[&id],
        )
        .await
        .map_err(|e| database_error(&continent, "read", e))?;

    let article = match row {
        Some(row) => Article::from_row(row),
        None => {
            log(&state, "read", &continent);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    log(&state, "read", &continent);
    Ok(Json(article))
}

async fn update_article(
    State(state): State<Arc<AppState>>,
    Path((continent, id)): Path<(String, i64)>,
    Json(article): Json<NewArticle>,
) -> Result<Json<Article>, StatusCode> {
    let shard = shard(&state, &continent)?;

    let row = shard
        .query_opt(
            "UPDATE articles SET title = $2, content = $3 WHERE id = $1 RETURNING id, continent, title, content, published_at",
            &[&id, &article.title, &article.content],
        )
        .await
        .map_err(|e| database_error(&continent, "update", e))?;

    let updated = match row {
        Some(row) => Article::from_row(row),
        None => {
            log(&state, "update", &continent);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    log(&state, "update", &continent);
    Ok(Json(updated))
}

async fn delete_article(
    State(state): State<Arc<AppState>>,
    Path((continent, id)): Path<(String, i64)>,
) -> Result<StatusCode, StatusCode> {
    let shard = shard(&state, &continent)?;

    let deleted = shard
        .execute("DELETE FROM articles WHERE id = $1", &[&id])
        .await
        .map_err(|e| database_error(&continent, "delete", e))?;

    if let Err(e) =
        cache::evict_cached_article(state.redis.clone(), &continent, id).await
    {
        eprintln!(
            "{} could not evict deleted article {}: {e}",
            state.instance, id
        );
    }

    log(&state, "delete", &continent);

    if deleted == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

fn shard<'a>(
    state: &'a AppState,
    continent: &str,
) -> Result<&'a Client, StatusCode> {
    state.shards.get(continent).ok_or_else(|| {
        eprintln!("{} rejected unknown continent {continent}", state.instance);
        StatusCode::BAD_REQUEST
    })
}

fn database_error(
    continent: &str,
    action: &str,
    error: tokio_postgres::Error,
) -> StatusCode {
    eprintln!("{action} in {continent} failed: {error}");
    StatusCode::INTERNAL_SERVER_ERROR
}

fn log(state: &AppState, action: &str, continent: &str) {
    println!("{} {action} article in {continent}", state.instance);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut shards = HashMap::new();

    for (continent, dbname) in CONTINENTS {
        let (client, connection) = tokio_postgres::connect(
            &format!(
                "host=postgres-articles-{continent} port=5432 dbname={dbname} user=articles password=articles"
            ),
            NoTls,
        )
        .await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("{} connection error: {}", continent, e);
            }
        });

        shards.insert(continent.to_string(), client);
    }

    let instance = std::env::var("HOSTNAME")
        .unwrap_or_else(|_| "article-service".to_string());

    let redis = redis::Client::open(REDIS_URL)?
        .get_multiplexed_async_connection()
        .await?;

    let instance_cloned = instance.clone();
    let state = Arc::new(AppState {
        shards,
        instance,
        redis,
    });

    filler::spawn(state.clone());

    let app = Router::new()
        .route("/continents/{continent}/articles", post(create_article))
        .route(
            "/continents/{continent}/articles/{id}",
            get(get_article).put(update_article).delete(delete_article),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("{instance_cloned} listening on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
