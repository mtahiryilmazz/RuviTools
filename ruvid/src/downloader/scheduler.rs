use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::error::{RuviError, RuviResult};

#[derive(Clone)]
pub struct DownloadScheduler {
    semaphore: Arc<Semaphore>,
}

impl DownloadScheduler {
    pub fn new(max_jobs: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_jobs.max(1))),
        }
    }

    pub async fn acquire(&self) -> RuviResult<tokio::sync::OwnedSemaphorePermit> {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| RuviError::Cancelled("scheduler closed".to_string()))
    }
}
