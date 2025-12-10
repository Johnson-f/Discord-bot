use std::env;

use redis::{aio::ConnectionManager, Client};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("redis url not set (REDIS_URL)")]
    MissingUrl,
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
}

#[derive(Clone)]
pub struct RedisCache {
    manager: ConnectionManager,
}

impl RedisCache {
    pub async fn new(url: &str) -> Result<Self, CacheError> {
        let client = Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self { manager })
    }

    pub async fn from_env() -> Result<Self, CacheError> {
        let url = env::var("REDIS_URL").map_err(|_| CacheError::MissingUrl)?;
        Self::new(&url).await
    }

    pub fn connection(&self) -> ConnectionManager {
        self.manager.clone()
    }
}

