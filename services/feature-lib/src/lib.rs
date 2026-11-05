use std::{
    sync::Arc,
    time::{
        Duration,
        Instant,
    },
};

use anyhow::bail;
use serde::Deserialize;
use tokio::sync::RwLock;

const FEATURE_CACHE_TTL: Duration = Duration::from_secs(5);
const FEATURE_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
pub struct FeatureHubResponse {
    pub features: Vec<Feature>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Feature {
    pub key: String,
    pub value: serde_json::Value,

    #[serde(rename = "type")]
    pub feature_type: String,
}

pub struct FeatureApiConfig {
    pub url: String,
    pub api_key: String,
}

#[derive(Debug, Default)]
struct FeatureCache {
    cached_features: Vec<Feature>,
    last_cache: Option<Instant>,
    last_lookup: Option<Instant>,
}

pub struct Features {
    config: FeatureApiConfig,
    cache: Arc<RwLock<FeatureCache>>,
}

impl Features {
    pub fn new(config: FeatureApiConfig) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(FeatureCache::default())),
        }
    }

    pub async fn get(&self, key: &str) -> anyhow::Result<Feature> {
        if self.cache_expired().await && self.should_try_lookup().await {
            let _ = self.try_fetch().await;
        }

        let cache = self.cache.read().await;

        match cache.cached_features.iter().find(|f| f.key == key).cloned() {
            Some(f) => Ok(f),
            None => {
                bail!("Could not find a feature with key `{key}`");
            }
        }
    }

    async fn try_fetch(&self) -> anyhow::Result<()> {
        {
            self.cache.write().await.last_lookup = Some(Instant::now());
        }

        const ATTEMPTS: u32 = 3;

        let client = reqwest::Client::new();
        let mut last_error = None;

        for attempt in 1..=ATTEMPTS {
            match self.fetch_once(&client).await {
                Ok(features) => {
                    let mut cache = self.cache.write().await;
                    cache.cached_features = features;
                    cache.last_cache = Some(Instant::now());
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);

                    if attempt < ATTEMPTS {
                        let timeout =
                            Duration::from_millis(100) * attempt.pow(2);
                        tokio::time::sleep(timeout).await;
                    }
                }
            }
        }

        match last_error {
            Some(e) => Err(e),
            None => bail!("Could not fetch features from FeatureHub"),
        }
    }

    async fn fetch_once(
        &self,
        client: &reqwest::Client,
    ) -> anyhow::Result<Vec<Feature>> {
        let response = client
            .get(format!(
                "{}/features?apiKey={}",
                self.config.url, self.config.api_key
            ))
            .send()
            .await?
            .error_for_status()?;

        let response: Vec<FeatureHubResponse> = response.json().await?;

        Ok(response.into_iter().flat_map(|r| r.features).collect())
    }

    async fn should_try_lookup(&self) -> bool {
        let last_lookup = self.cache.read().await.last_lookup;

        last_lookup.is_none_or(|c| {
            Instant::now().duration_since(c) > FEATURE_LOOKUP_TIMEOUT
        })
    }

    async fn cache_expired(&self) -> bool {
        let last_cache = self.cache.read().await.last_cache;

        last_cache.is_none_or(|c| {
            Instant::now().duration_since(c) > FEATURE_CACHE_TTL
        })
    }
}
