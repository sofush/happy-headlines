use std::sync::Arc;

use axum::{
    Json,
    Router,
    extract::{
        Query,
        State,
    },
    http::StatusCode,
    routing::{
        get,
        post,
    },
};
use redis::AsyncCommands as _;
use tokio_postgres::{
    Client,
    NoTls,
};

use crate::subscriber::{
    DeleteSubscriber,
    Subscriber,
};

const SUBSCRIBER_QUEUE: &str = "subscriber:queue";

mod subscriber;

struct AppState {
    db: Client,
    redis: redis::aio::MultiplexedConnection,
}

async fn post_subscriber(
    State(state): State<Arc<AppState>>,
    Json(subscriber): Json<Subscriber>,
) -> Result<(StatusCode, Json<Subscriber>), StatusCode> {
    let row = state
        .db
        .query_opt(
            r#"
            INSERT INTO subscribers (email)
            VALUES ($1)
            ON CONFLICT (email) DO NOTHING
            RETURNING email
            "#,
            &[&subscriber.email],
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let Some(row) = row else {
        return Err(StatusCode::CONFLICT);
    };

    let subscriber = Subscriber {
        email: row.get("email"),
    };

    let encoded = serde_json::to_string(&subscriber)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut redis = state.redis.clone();
    redis
        .rpush::<_, _, ()>(SUBSCRIBER_QUEUE, encoded)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(subscriber)))
}

async fn delete_subscriber(
    State(state): State<Arc<AppState>>,
    Query(subscriber): Query<DeleteSubscriber>,
) -> Result<StatusCode, StatusCode> {
    let deleted = state
        .db
        .execute(
            "DELETE FROM subscribers WHERE email = $1",
            &[&subscriber.email],
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if deleted == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    let encoded = serde_json::to_string(&Subscriber {
        email: subscriber.email,
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut redis = state.redis.clone();
    redis
        .lrem::<_, _, ()>(SUBSCRIBER_QUEUE, 0, encoded)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn take_new_subscribers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Subscriber>>, StatusCode> {
    let mut redis = state.redis.clone();
    let mut subscribers = Vec::new();

    loop {
        let raw = redis
            .lpop::<_, Option<String>>(SUBSCRIBER_QUEUE, None)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let Some(raw) = raw else {
            break;
        };

        match serde_json::from_str::<Subscriber>(&raw) {
            Ok(subscriber) => subscribers.push(subscriber),
            Err(e) => eprintln!("skipping malformed queue entry: {e}"),
        }
    }

    Ok(Json(subscribers))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (client, connection) = tokio_postgres::connect(
        "host=postgres-subscribers port=5432 dbname=subscribers user=subscribers password=subscribers",
        NoTls,
    )
    .await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let redis = redis::Client::open("redis://redis-subscriber-queue:6379/")?;

    let state = Arc::new(AppState {
        db: client,
        redis: redis.get_multiplexed_async_connection().await?,
    });

    let app = Router::new()
        .route(
            "/subscriber",
            post(post_subscriber).delete(delete_subscriber),
        )
        .route("/subscriber/new", get(take_new_subscribers))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3003").await?;
    println!("Listening on http://localhost:3003");
    axum::serve(listener, app).await?;

    Ok(())
}
