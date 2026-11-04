use std::time::Duration;

use anyhow::bail;
use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FeatureHubResponse {
    pub features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
pub struct Feature {
    pub key: String,
    pub value: serde_json::Value,

    #[serde(rename = "type")]
    pub feature_type: String,
}

async fn get_features() -> anyhow::Result<Vec<FeatureHubResponse>> {
    let feature_hub_url = std::env::var("FEATUREHUB_URL")?;
    let api_key = std::env::var("FEATUREHUB_API_KEY")?;
    let client = reqwest::Client::new();
    let attempts = 3u32;

    let do_timeout = async |idx: u32| {
        let timeout = Duration::from_millis(100) * idx.pow(2);
        let _ = tokio::time::sleep(timeout).await;
    };

    for i in 1..=attempts {
        let Ok(response) = client
            .get(format!("{}/features?apiKey={}", feature_hub_url, api_key))
            .send()
            .await
        else {
            do_timeout(i).await;
            continue;
        };

        let success = response.status() == StatusCode::OK;

        match response.json::<Vec<FeatureHubResponse>>().await {
            Ok(o) if success => return Ok(o),
            Err(e) if i == attempts => bail!(e),
            _ => do_timeout(i).await,
        }
    }

    bail!("unreachable")
}

pub async fn should_send_emails() -> anyhow::Result<bool> {
    let feature_name = "send-emails";
    let groups = get_features().await?;

    for group in groups {
        for feature in group.features {
            if feature.feature_type != "BOOLEAN" {
                continue;
            }

            if feature.key != feature_name {
                continue;
            }

            if let Some(b) = feature.value.as_bool() {
                return Ok(b);
            }
        }
    }

    bail!("Could not find a feature with the name `{feature_name}`")
}
