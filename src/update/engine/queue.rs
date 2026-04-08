use super::heuristics::{insert_download_job_sorted, is_large_download};
use super::types::UpdateDownloadJob;
use std::collections::{HashMap, VecDeque};

pub struct DownloadQueues {
    pub(crate) normal: VecDeque<UpdateDownloadJob>,
    pub(crate) large: VecDeque<UpdateDownloadJob>,
}

impl DownloadQueues {
    pub fn new() -> Self {
        Self {
            normal: VecDeque::new(),
            large: VecDeque::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.normal.is_empty() && self.large.is_empty()
    }

    pub fn len(&self) -> usize {
        self.normal.len() + self.large.len()
    }

    pub fn push(&mut self, job: UpdateDownloadJob) {
        if job.retry_without_segmented_downloads {
            self.large.push_back(job);
            return;
        }

        if is_large_download(job.estimated_download_bytes) {
            insert_download_job_sorted(&mut self.large, job);
        } else {
            insert_download_job_sorted(&mut self.normal, job);
        }
    }

    pub fn pop_next(&mut self, allow_large: bool) -> Option<UpdateDownloadJob> {
        if let Some(job) = self.normal.pop_front() {
            return Some(job);
        }

        if allow_large {
            return self.large.pop_front();
        }

        None
    }
}

pub struct ProviderDownloadScheduler {
    active_global: usize,
    active_by_provider: HashMap<String, usize>,
}

impl Default for ProviderDownloadScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderDownloadScheduler {
    pub fn new() -> Self {
        Self {
            active_global: 0,
            active_by_provider: HashMap::new(),
        }
    }

    fn provider_limit(provider: &str) -> usize {
        if provider == "github" { 3 } else { 2 }
    }

    pub fn try_acquire(&mut self, provider: &str, global_limit: usize) -> bool {
        if self.active_global >= global_limit {
            return false;
        }

        let provider_limit = Self::provider_limit(provider);
        let active_for_provider = *self.active_by_provider.get(provider).unwrap_or(&0);
        if active_for_provider >= provider_limit {
            return false;
        }

        self.active_global += 1;
        *self
            .active_by_provider
            .entry(provider.to_string())
            .or_insert(0) += 1;
        true
    }

    pub fn release(&mut self, provider: &str) {
        if self.active_global > 0 {
            self.active_global -= 1;
        }
        if let Some(active_for_provider) = self.active_by_provider.get_mut(provider) {
            if *active_for_provider > 1 {
                *active_for_provider -= 1;
            } else {
                self.active_by_provider.remove(provider);
            }
        }
    }
}

pub enum UpdateErrorStage {
    Check,
    Download,
}
