use std::{
    collections::HashMap,
    sync::Arc,
};

use axum::{
    Router,
    routing::{
        get,
        post,
    },
};
use axum_prometheus::PrometheusMetricLayer;
use tokio_postgres::{
    Client,
    NoTls,
};
use tower_http::{
    compression::CompressionLayer,
    trace::TraceLayer,
};

use crate::{
    cache::ArticleCache,
    controller::{
        create_article,
        delete_article,
        get_article,
        update_article,
    },
    repository::ArticleRepository,
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
mod controller;
mod repository;
mod telemetry;

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

    tracing::info!("shutdown requested");
}

async fn connect_shards() -> anyhow::Result<HashMap<String, Client>> {
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

    Ok(shards)
}

async fn serve(instance: &str) -> anyhow::Result<()> {
    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();

    let shards = connect_shards().await?;

    let redis = redis::Client::open(REDIS_URL)?
        .get_multiplexed_async_connection()
        .await?;

    let repository =
        Arc::new(ArticleRepository::new(shards, ArticleCache::new(redis)));

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
        .layer(CompressionLayer::new())
        .with_state(repository);

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
