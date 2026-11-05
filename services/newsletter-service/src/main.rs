use std::sync::Arc;

use axum::{
    Json,
    Router,
    extract::State,
    http::StatusCode,
    routing::post,
};
use feature_lib::{
    FeatureApiConfig,
    Features,
};

const SUBSCRIBER_SERVICE_URL: &str = "http://subscriber-service:3003";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct Subscriber {
    email: String,
}

#[derive(serde::Serialize)]
struct NewsletterRun {
    sent: usize,
}

struct AppState {
    http: reqwest::Client,
    features: Features,
}

async fn get_boolean_feature(
    features: &Features,
    key: &str,
    default: bool,
) -> bool {
    let Ok(feature) = features.get(key).await else {
        return default;
    };

    if let Some(b) = feature.value.as_bool() {
        return b;
    }

    default
}

async fn should_send_emails(features: &Features) -> bool {
    get_boolean_feature(features, "send-emails", false).await
}

async fn send_newsletters(
    State(state): State<Arc<AppState>>,
) -> Result<Json<NewsletterRun>, StatusCode> {
    if !should_send_emails(&state.features).await {
        return Ok(Json(NewsletterRun { sent: 0 }));
    }

    let response = state
        .http
        .get(format!("{SUBSCRIBER_SERVICE_URL}/subscriber/new"))
        .send()
        .await
        .map_err(|e| {
            eprintln!("could not fetch subscribers: {e}");
            StatusCode::BAD_GATEWAY
        })?;

    if !response.status().is_success() {
        eprintln!("subscriber service replied {}", response.status());
        return Err(StatusCode::BAD_GATEWAY);
    }

    let subscribers =
        response.json::<Vec<Subscriber>>().await.map_err(|e| {
            eprintln!("could not read subscribers: {e}");
            StatusCode::BAD_GATEWAY
        })?;

    eprintln!("Pretending to send emails...");

    for subscriber in &subscribers {
        send_welcome_mail(&subscriber.email).await;
    }

    Ok(Json(NewsletterRun {
        sent: subscribers.len(),
    }))
}

async fn send_welcome_mail(_email: &str) {
    // TODO: send the welcome email through an email provider.
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let features = Features::new(FeatureApiConfig {
        url: std::env::var("FEATUREHUB_URL")
            .unwrap_or(String::from("http://featurehub:8085")),
        api_key: std::env::var("FEATUREHUB_API_KEY")
            .unwrap_or(String::from("b29c5fa4-0e2b-4373-8cbb-e8ab72df4275/Kq4d2EE7UAaHKjQz4JGWQ6L7HnP07t*nAoDknnCWGjBgNmjIwGe")),
    });

    let state = Arc::new(AppState {
        http: reqwest::Client::new(),
        features,
    });

    let app = Router::new()
        .route("/newsletter", post(send_newsletters))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3004").await?;
    println!("Listening on http://localhost:3004");
    axum::serve(listener, app).await?;

    Ok(())
}
