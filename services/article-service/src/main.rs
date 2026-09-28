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

struct AppState {
    shards: HashMap<String, Client>,
    instance: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Article {
    id: i64,
    continent: String,
    title: String,
    content: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct NewArticle {
    title: String,
    content: String,
}

async fn create_article(
    State(state): State<Arc<AppState>>,
    Path(continent): Path<String>,
    Json(article): Json<NewArticle>,
) -> Result<(StatusCode, Json<Article>), StatusCode> {
    let shard = shard(&state, &continent)?;

    let row = shard
        .query_one(
            "INSERT INTO articles (continent, title, content) VALUES ($1, $2, $3) RETURNING id, continent, title, content",
            &[&continent, &article.title, &article.content],
        )
        .await
        .map_err(|e| database_error(&continent, "create", e))?;

    log(&state, "create", &continent);

    Ok((StatusCode::CREATED, Json(article_from(row))))
}

async fn get_article(
    State(state): State<Arc<AppState>>,
    Path((continent, id)): Path<(String, i64)>,
) -> Result<Json<Article>, StatusCode> {
    let shard = shard(&state, &continent)?;

    let row = shard
        .query_opt(
            "SELECT id, continent, title, content FROM articles WHERE id = $1",
            &[&id],
        )
        .await
        .map_err(|e| database_error(&continent, "read", e))?;

    log(&state, "read", &continent);

    match row {
        Some(row) => Ok(Json(article_from(row))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn update_article(
    State(state): State<Arc<AppState>>,
    Path((continent, id)): Path<(String, i64)>,
    Json(article): Json<NewArticle>,
) -> Result<Json<Article>, StatusCode> {
    let shard = shard(&state, &continent)?;

    let row = shard
        .query_opt(
            "UPDATE articles SET title = $2, content = $3 WHERE id = $1 RETURNING id, continent, title, content",
            &[&id, &article.title, &article.content],
        )
        .await
        .map_err(|e| database_error(&continent, "update", e))?;

    log(&state, "update", &continent);

    match row {
        Some(row) => Ok(Json(article_from(row))),
        None => Err(StatusCode::NOT_FOUND),
    }
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

    log(&state, "delete", &continent);

    if deleted == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

fn shard<'a>(state: &'a AppState, continent: &str) -> Result<&'a Client, StatusCode> {
    state.shards.get(continent).ok_or_else(|| {
        eprintln!("{} rejected unknown continent {continent}", state.instance);
        StatusCode::BAD_REQUEST
    })
}

fn article_from(row: tokio_postgres::Row) -> Article {
    Article {
        id: row.get("id"),
        continent: row.get("continent"),
        title: row.get("title"),
        content: row.get("content"),
    }
}

fn database_error(continent: &str, action: &str, error: tokio_postgres::Error) -> StatusCode {
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

    let instance = std::env::var("HOSTNAME").unwrap_or_else(|_| "article-service".to_string());

    let instance_cloned = instance.clone();
    let state = Arc::new(AppState { shards, instance });

    let app = Router::new()
        .route("/continents/{continent}/articles", post(create_article))
        .route(
            "/continents/{continent}/articles/{id}",
            get(get_article).put(update_article).delete(delete_article),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002")
        .await
        .unwrap();
    println!("{instance_cloned} listening on http://localhost:3002");
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
