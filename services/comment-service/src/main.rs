use std::sync::Arc;

use axum::{
    Json,
    Router,
    extract::State,
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

#[derive(serde::Serialize, Debug)]
struct Comment {
    author: String,
    content: String,
}

async fn post_comment(Json(comment): Json<serde_json::Value>) {}

async fn get_comments(
    State(db): State<Arc<Client>>,
) -> Result<Json<Vec<Comment>>, StatusCode> {
    let rows = db
        .query("SELECT author, content FROM comments", &[])
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let comments: Vec<Comment> = rows
        .iter()
        .map(|r| Comment {
            author: r.get("author"),
            content: r.get("content"),
        })
        .collect();

    Ok(Json(comments))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (client, connection) = tokio_postgres::connect(
        "host=localhost port=5432 dbname=comments user=comments password=comments",
        NoTls,
    )
    .await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let app = Router::new()
        .route("/comments", post(post_comment))
        .route("/comments", get(get_comments))
        .with_state(Arc::new(client));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
