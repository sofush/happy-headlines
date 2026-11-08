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
use axum_prometheus::PrometheusMetricLayer;
use metrics::Counter;
use std::sync::LazyLock;
use tokio_postgres::{
    Client,
    NoTls,
};
use tower_http::trace::TraceLayer;

use crate::{
    article::{
        Article,
        NewArticle,
    },
    telemetry::{
        HttpMakeSpan,
        HttpOnResponse,
    },
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
mod telemetry;

static ARTICLE_CACHE_HITS_TOTAL: LazyLock<Counter> =
    LazyLock::new(|| metrics::counter!("article_cache_hits_total"));

static ARTICLE_CACHE_MISSES_TOTAL: LazyLock<Counter> =
    LazyLock::new(|| metrics::counter!("article_cache_misses_total"));

pub struct AppState {
    pub shards: HashMap<String, Client>,
    pub instance: String,
    pub redis: redis::aio::MultiplexedConnection,
}

#[tracing::instrument(
    level = "INFO",
    skip(state, article),
    fields(continent = %continent)
)]
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
        tracing::warn!(
            continent = %continent,
            article.id = article.id,
            error = %e,
            "could not cache created article"
        );
    }

    Ok((StatusCode::CREATED, Json(article)))
}

#[tracing::instrument(
    level = "INFO",
    skip(state),
    fields(continent = %continent, id = %id, cache = tracing::field::Empty)
)]
async fn get_article(
    State(state): State<Arc<AppState>>,
    Path((continent, id)): Path<(String, i64)>,
) -> Result<Json<Article>, StatusCode> {
    let shard = shard(&state, &continent)?;

    if let Ok(article) =
        cache::get_cached_article(state.redis.clone(), &continent, id).await
    {
        tracing::Span::current().record("cache", "hit");
        ARTICLE_CACHE_HITS_TOTAL.increment(1);
        return Ok(Json(article));
    }

    tracing::Span::current().record("cache", "miss");
    ARTICLE_CACHE_MISSES_TOTAL.increment(1);

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
            tracing::debug!(continent = %continent, id = %id, "article not found");
            return Err(StatusCode::NOT_FOUND);
        }
    };

    Ok(Json(article))
}

#[tracing::instrument(
    level = "INFO",
    skip(state, article),
    fields(continent = %continent, id = %id)
)]
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
            tracing::debug!(
                continent = %continent,
                id = %id,
                "article not found"
            );
            return Err(StatusCode::NOT_FOUND);
        }
    };

    Ok(Json(updated))
}

#[tracing::instrument(level = "INFO", skip(state), fields(continent = %continent, id = %id))]
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
        tracing::warn!(
            continent = %continent,
            id = %id,
            error = %e,
            "could not evict deleted article"
        );
    }

    if deleted == 0 {
        tracing::debug!(continent = %continent, id = %id, "article not found");
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

fn shard<'a>(
    state: &'a AppState,
    continent: &str,
) -> Result<&'a Client, StatusCode> {
    state.shards.get(continent).ok_or_else(|| {
        tracing::warn!(continent, "rejected request for unknown continent");
        StatusCode::BAD_REQUEST
    })
}

fn database_error(
    continent: &str,
    action: &str,
    error: tokio_postgres::Error,
) -> StatusCode {
    tracing::error!(continent, action, error = %error, "database query failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        ) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }

    tracing::info!("shutdown requested, draining in-flight requests");
}

async fn serve(instance: &str) -> anyhow::Result<()> {
    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();

    let mut shards = HashMap::new();

    for (continent, dbname) in CONTINENTS {
        let (client, connection) = tokio_postgres::connect(
            &format!(
                "host=postgres-articles-{continent} port=5432 dbname={dbname} user=articles password=articles"
            ),
            NoTls,
        )
        .await?;

        let owned = continent.to_string();
        let watched = owned.clone();

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(continent = %watched, error = %e, "database connection lost");
            }
        });

        shards.insert(owned, client);
    }

    let redis = redis::Client::open(REDIS_URL)?
        .get_multiplexed_async_connection()
        .await?;

    let state = Arc::new(AppState {
        shards,
        instance: instance.to_string(),
        redis,
    });

    filler::spawn(state.clone());

    let app = Router::new()
        .route("/continents/{continent}/articles", post(create_article))
        .route(
            "/continents/{continent}/articles/{id}",
            get(get_article).put(update_article).delete(delete_article),
        )
        .route("/metrics", get(|| async move { metric_handle.render() }))
        .layer(prometheus_layer)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(HttpMakeSpan)
                .on_response(HttpOnResponse),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!(instance, address = %listener.local_addr()?, "article-service listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let instance = std::env::var("HOSTNAME")
        .unwrap_or_else(|_| "article-service".to_string());

    telemetry::init(&instance)?;

    let result = serve(&instance).await;

    if let Err(e) = &result {
        tracing::error!(instance, error = %format!("{e:#}"), "article-service stopped");
    }

    telemetry::shutdown();

    result
}
