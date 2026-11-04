use std::sync::Arc;

use axum::{
    Json,
    Router,
    extract::State,
    http::StatusCode,
    routing::post,
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
}

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FeatureHubResponse {
    pub id: String,
    pub features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
pub struct Feature {
    pub id: String,
    pub key: String,
    pub value: serde_json::Value,
    pub version: u32,

    #[serde(rename = "l", default)]
    pub locked: bool,

    #[serde(rename = "type")]
    pub feature_type: String,
}

async fn should_send_emails() -> anyhow::Result<bool> {
    let feature_hub_url = std::env::var("FEATUREHUB_URL")?;
    let api_key = std::env::var("FEATUREHUB_API_KEY")?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/features?apiKey={}", feature_hub_url, api_key))
        .send()
        .await?
        .error_for_status()?;

    let response: Vec<FeatureHubResponse> = response.json().await?;

    for item in response {
        for feature in item.features {
            if feature.feature_type != "BOOLEAN" {
                continue;
            }

            if feature.key != "send-emails" {
                continue;
            }

            return Ok(feature.value.as_bool().unwrap_or(true));
        }
    }

    Ok(true)
}

async fn send_newsletters(
    State(state): State<Arc<AppState>>,
) -> Result<Json<NewsletterRun>, StatusCode> {
    if should_send_emails().await.is_ok_and(|b| !b) {
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
    let state = Arc::new(AppState {
        http: reqwest::Client::new(),
    });

    let app = Router::new()
        .route("/newsletter", post(send_newsletters))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3004").await?;
    println!("Listening on http://localhost:3004");
    axum::serve(listener, app).await?;

    Ok(())
}
