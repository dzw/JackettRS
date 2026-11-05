use std::collections::VecDeque;
use std::sync::Mutex;

use crate::models::cached_log::CachedLog;

/// Mirror of Jackett.Common/Services/LogCacheService.cs
pub struct LogCacheService {
    logs: Mutex<VecDeque<CachedLog>>,
    capacity: usize,
}

impl LogCacheService {
    pub fn new(capacity: usize) -> Self {
        Self {
            logs: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    /// Mirror of `Add`.
    pub fn add(&self, level: impl Into<String>, message: impl Into<String>) {
        let mut logs = self.logs.lock().unwrap();
        if logs.len() >= self.capacity {
            logs.pop_front();
        }
        logs.push_back(CachedLog {
            level: Some(level.into()),
            message: Some(message.into()),
            when: chrono::Local::now(),
        });
    }

    /// Mirror of `Logs`.
    pub fn logs(&self) -> Vec<CachedLog> {
        self.logs.lock().unwrap().iter().cloned().collect()
    }
}
