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
use axum_prometheus::PrometheusMetricLayer;
use metrics::Counter;
use reqwest::Client as ProfanityClient;
use std::sync::LazyLock;
use tokio_postgres::{
    Client,
    NoTls,
};

use crate::comment::{
    Comment,
    CommentQuery,
    MaskComment,
    NewComment,
};

const PROFANITY_SERVICE_URL: &str = "http://profanity-service:3002";
const PROFANITY_TIMEOUT: Duration = Duration::from_millis(500);
const FALLBACK_PROFANITY: &[&str] = &["crap", "damn", "shit"];

static COMMENT_CACHE_HITS_TOTAL: LazyLock<Counter> =
    LazyLock::new(|| metrics::counter!("comment_cache_hits_total"));

static COMMENT_CACHE_MISSES_TOTAL: LazyLock<Counter> =
    LazyLock::new(|| metrics::counter!("comment_cache_misses_total"));

mod cache;
mod comment;

struct AppState {
    redis: redis::aio::MultiplexedConnection,
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
            r#"
            INSERT INTO comments (continent, article_id, author, content)
            VALUES ($1, $2, $3, $4)
            RETURNING id, continent, article_id, author, content
            "#,
            &[
                &comment.continent,
                &comment.article_id,
                &mask.author,
                &mask.content,
            ],
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
    if let Ok(comments) =
        crate::cache::get_cached_comments(state.redis.clone(), &query).await
    {
        COMMENT_CACHE_HITS_TOTAL.increment(1);
        return Ok(Json(comments));
    }

    COMMENT_CACHE_MISSES_TOTAL.increment(1);

    let sql = r#"
         SELECT id, continent, article_id, author, content
         FROM comments
         WHERE article_id = $1 AND continent = $2
         ORDER BY id
         "#;

    let rows = state
        .db
        .query(sql, &[&query.article_id, &query.continent])
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

    crate::cache::store_cached_comments(
        state.redis.clone(),
        &query,
        comments.clone(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();

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

    let redis = redis::Client::open("redis://redis-cache:6379/")?;

    let state = Arc::new(AppState {
        redis: redis.get_multiplexed_async_connection().await?,
        db: client,
        profanity,
    });

    let app = Router::new()
        .route("/comments", post(post_comment))
        .route("/comments", get(get_comments))
        .route("/metrics", get(|| async move { metric_handle.render() }))
        .layer(prometheus_layer)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    println!("Listening on http://localhost:3001");
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
