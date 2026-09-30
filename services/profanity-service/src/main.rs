use std::sync::Arc;

use axum::{
    Json,
    Router,
    extract::State,
    http::StatusCode,
    routing::post,
};
use tokio_postgres::{
    Client,
    NoTls,
};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Comment {
    author: String,
    content: String,
}

struct AppState {
    db: Client,
}

async fn mask_comment(
    State(state): State<Arc<AppState>>,
    Json(comment): Json<Comment>,
) -> Result<Json<Comment>, StatusCode> {
    let rows = state
        .db
        .query("SELECT word FROM words", &[])
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let words: Vec<String> = rows.iter().map(|row| row.get("word")).collect();

    Ok(Json(Comment {
        author: comment.author,
        content: mask_words(&comment.content, &words),
    }))
}

fn mask_words(text: &str, words: &[String]) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut rest = text;

    while !rest.is_empty() {
        let matched = words
            .iter()
            .filter(|word| {
                !word.is_empty()
                    && rest.len() >= word.len()
                    && rest.as_bytes()[..word.len()]
                        .eq_ignore_ascii_case(word.as_bytes())
            })
            .max_by_key(|word| word.len());

        match matched {
            Some(word) => {
                masked.push_str(&"*".repeat(word.chars().count()));
                rest = &rest[word.len()..];
            }
            None => {
                let c = rest.chars().next().expect("rest is not empty");
                masked.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }

    masked
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (client, connection) = tokio_postgres::connect(
        "host=postgres-profanity port=5432 dbname=profanity user=profanity password=profanity",
        NoTls,
    )
    .await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let app = Router::new()
        .route("/mask", post(mask_comment))
        .with_state(Arc::new(AppState { db: client }));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002").await.unwrap();
    println!("Listening on http://localhost:3002");
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
