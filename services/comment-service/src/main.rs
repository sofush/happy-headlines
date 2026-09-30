use std::{
    sync::Arc,
    time::Duration,
};

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
use reqwest::Client as ProfanityClient;
use tokio_postgres::{
    Client,
    NoTls,
};

const PROFANITY_SERVICE_URL: &str = "http://profanity-service:3001";
const PROFANITY_TIMEOUT: Duration = Duration::from_millis(500);
const FALLBACK_PROFANITY: &[&str] = &["crap", "damn", "shit"];

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Comment {
    id: i64,
    continent: String,
    article_id: i64,
    author: String,
    content: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct NewComment {
    continent: String,
    article_id: i64,
    author: String,
    content: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct MaskComment {
    author: String,
    content: String,
}

#[derive(serde::Deserialize)]
struct CommentQuery {
    continent: Option<String>,
    article_id: Option<i64>,
}

struct AppState {
    db: Client,
    profanity: ProfanityClient,
}

async fn post_comment(
    State(state): State<Arc<AppState>>,
    Json(comment): Json<NewComment>,
) -> Result<(StatusCode, Json<Comment>), StatusCode> {
    let mask = MaskComment {
        author: comment.author,
        content: comment.content,
    };

    let mask = mask_profanity(&state.profanity, &mask).await;

    let row = state
        .db
        .query_one(
            "INSERT INTO comments (continent, article_id, author, content) VALUES ($1, $2, $3, $4) RETURNING id, continent, article_id, author, content",
            &[&comment.continent, &comment.article_id, &mask.author, &mask.content],
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let comment = Comment {
        id: row.get("id"),
        continent: row.get("continent"),
        article_id: row.get("article_id"),
        author: row.get("author"),
        content: row.get("content"),
    };

    Ok((StatusCode::CREATED, Json(comment)))
}

async fn get_comments(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CommentQuery>,
) -> Result<Json<Vec<Comment>>, StatusCode> {
    let mut sql = String::from(
        "SELECT id, continent, article_id, author, content FROM comments",
    );
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();

    let article_id: Option<i64> = query.article_id;
    let continent: Option<String> = query.continent;

    if let Some(id) = &article_id {
        params.push(id);
        conditions.push(format!("article_id = ${}", params.len()));
    }

    if let Some(name) = &continent {
        params.push(name);
        conditions.push(format!("continent = ${}", params.len()));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }

    sql.push_str(" ORDER BY id");

    let rows = state
        .db
        .query(&sql, &params)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let comments: Vec<Comment> = rows
        .iter()
        .map(|r| Comment {
            id: r.get("id"),
            continent: r.get("continent"),
            article_id: r.get("article_id"),
            author: r.get("author"),
            content: r.get("content"),
        })
        .collect();

    Ok(Json(comments))
}

async fn mask_profanity(
    profanity: &ProfanityClient,
    comment: &MaskComment,
) -> MaskComment {
    let content = remote_mask(profanity, comment)
        .await
        .unwrap_or_else(|| mask_locally(&comment.content));

    MaskComment {
        author: comment.author.clone(),
        content,
    }
}

async fn remote_mask(
    profanity: &ProfanityClient,
    comment: &MaskComment,
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

    match response.json::<MaskComment>().await {
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
        "host=postgres-comments port=5432 dbname=comments user=comments password=comments",
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