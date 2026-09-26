use std::{
    sync::Arc,
    time::Duration,
};

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
use reqwest::Client as ProfanityClient;
use tokio_postgres::{
    Client,
    NoTls,
};

const PROFANITY_SERVICE_URL: &str = "http://localhost:3001";
const PROFANITY_TIMEOUT: Duration = Duration::from_millis(500);
const FALLBACK_PROFANITY: &[&str] = &["crap", "damn", "shit"];

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Comment {
    author: String,
    content: String,
}

struct AppState {
    db: Client,
    profanity: ProfanityClient,
}

async fn post_comment(
    State(state): State<Arc<AppState>>,
    Json(comment): Json<Comment>,
) -> Result<(StatusCode, Json<Comment>), StatusCode> {
    let comment = mask_profanity(&state.profanity, &comment).await;

    let row = state
        .db
        .query_one(
            "INSERT INTO comments (author, content) VALUES ($1, $2) RETURNING author, content",
            &[&comment.author, &comment.content],
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let comment = Comment {
        author: row.get("author"),
        content: row.get("content"),
    };

    Ok((StatusCode::CREATED, Json(comment)))
}

async fn get_comments(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Comment>>, StatusCode> {
    let rows = state
        .db
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

async fn mask_profanity(
    profanity: &ProfanityClient,
    comment: &Comment,
) -> Comment {
    let content = remote_mask(profanity, comment)
        .await
        .unwrap_or_else(|| mask_locally(&comment.content));

    Comment {
        author: comment.author.clone(),
        content,
    }
}

async fn remote_mask(
    profanity: &ProfanityClient,
    comment: &Comment,
) -> Option<String> {
    let response = profanity
        .post(format!("{PROFANITY_SERVICE_URL}/mask"))
        .json(comment)
        .send()
        .await
        .map_err(|e| {
            eprintln!("profanity service unavailable ({e}), masking locally")
        })
        .ok()?;

    if !response.status().is_success() {
        eprintln!(
            "profanity service replied {}, masking locally",
            response.status()
        );
        return None;
    }

    match response.json::<Comment>().await {
        Ok(masked) => Some(masked.content),
        Err(e) => {
            eprintln!(
                "profanity service sent an unreadable body ({e}), masking locally"
            );
            None
        }
    }
}

fn mask_locally(content: &str) -> String {
    content
        .split_whitespace()
        .map(|token| {
            let bare = token.trim_matches(|c: char| !c.is_alphanumeric());

            if FALLBACK_PROFANITY
                .iter()
                .any(|word| bare.eq_ignore_ascii_case(word))
            {
                let stars = "*".repeat(bare.chars().count());
                token.replacen(bare, &stars, 1)
            } else {
                token.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    let profanity = ProfanityClient::builder()
        .timeout(PROFANITY_TIMEOUT)
        .connect_timeout(PROFANITY_TIMEOUT)
        .build()?;

    let state = Arc::new(AppState {
        db: client,
        profanity,
    });

    let app = Router::new()
        .route("/comments", post(post_comment))
        .route("/comments", get(get_comments))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
