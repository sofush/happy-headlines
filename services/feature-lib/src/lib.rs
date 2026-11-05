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
        eprintln!("key: {key}");

        if self.cache_expired().await && self.should_try_lookup().await {
            eprintln!("try fetch");
            let _ = self.try_fetch().await;
        }

        eprintln!("getting from cache");

        let cache = self.cache.read().await;

        match cache.cached_features.iter().find(|f| f.key == key).cloned() {
            Some(f) => Ok(f),
            None => {
                eprintln!("could not find the key");
                bail!("Could not find a feature with key `{key}`");
            }
        }
    }

    async fn try_fetch(&self) -> anyhow::Result<()> {
        {
            self.cache.write().await.last_lookup = Some(Instant::now());
        }

        let attempts = 3;

        let do_timeout = async |idx: u32| {
            eprintln!("timeout {idx}");
            let timeout = Duration::from_millis(100) * idx.pow(2);
            let _ = tokio::time::sleep(timeout).await;
        };

        let client = reqwest::Client::new();

        for i in 1..=attempts {
            eprintln!("attempt {i}");
            let Ok(response) = client
                .get(format!(
                    "{}/features?apiKey={}",
                    self.config.url, self.config.api_key
                ))
                .send()
                .await
            else {
                do_timeout(i).await;
                continue;
            };

            if !response.status().is_success() {
                do_timeout(i).await;
                continue;
            }

            eprintln!("updating cache");
            let mut cache = self.cache.write().await;
            let response: Vec<FeatureHubResponse> = response.json().await?;
            cache.cached_features =
                response.into_iter().flat_map(|r| r.features).collect();
            cache.last_cache = Some(Instant::now());
            eprintln!("cache: {cache:?}");
            return Ok(());
        }

        Ok(())
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
