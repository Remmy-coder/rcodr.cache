use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub evictions: usize,
    pub total_items_stored: usize,
    pub created_at: Instant,
    pub last_access: Instant,
    pub memory_usage: usize,
}

#[derive(Serialize, Deserialize, Default)]
pub struct SerializedCacheStats {
    hits: usize,
    misses: usize,
    evictions: usize,
    total_items_stored: usize,
    created_at: u64,
    last_access: u64,
    memory_usage: usize,
}

impl CacheStats {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            hits: 0,
            misses: 0,
            evictions: 0,
            total_items_stored: 0,
            created_at: now,
            last_access: now,
            memory_usage: 0,
        }
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }

    pub fn record_hit(&mut self) {
        self.hits += 1;
        self.last_access = Instant::now();
    }

    pub fn record_miss(&mut self) {
        self.misses += 1;
        self.last_access = Instant::now();
    }

    pub fn record_eviction(&mut self) {
        self.evictions += 1;
    }

    pub fn update_memory_usage(&mut self, bytes: usize) {
        self.memory_usage = bytes;
    }

    pub fn uptime(&self) -> u64 {
        self.created_at.elapsed().as_secs()
    }
}

impl Serialize for CacheStats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let now_instant = Instant::now();
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(serde::ser::Error::custom)?;

        let created_duration = since_epoch
            .checked_sub(now_instant.duration_since(self.created_at))
            .unwrap_or(Duration::ZERO);

        let last_access_duration = since_epoch
            .checked_sub(now_instant.duration_since(self.last_access))
            .unwrap_or(Duration::ZERO);

        let serializable = SerializedCacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            total_items_stored: self.total_items_stored,
            created_at: created_duration.as_secs(),
            last_access: last_access_duration.as_secs(),
            memory_usage: self.memory_usage,
        };

        serializable.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CacheStats {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let serializable = SerializedCacheStats::deserialize(deserializer)?;

        let now_instant = Instant::now();
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(serde::de::Error::custom)?;

        let created_at = now_instant - (since_epoch - Duration::from_secs(serializable.created_at));

        let last_access =
            now_instant - (since_epoch - Duration::from_secs(serializable.last_access));

        Ok(CacheStats {
            hits: serializable.hits,
            misses: serializable.misses,
            evictions: serializable.evictions,
            total_items_stored: serializable.total_items_stored,
            created_at,
            last_access,
            memory_usage: serializable.memory_usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;
    use std::thread::{self, sleep};

    #[test]
    fn test_serialization_cache_stats() {
        let mut stats = CacheStats::new();

        stats.hits = 5;
        stats.misses = 3;
        stats.evictions = 1;
        stats.total_items_stored = 10;
        stats.memory_usage = 1024;

        thread::sleep(Duration::from_millis(10));

        let serialized = serde_json::to_string(&stats).unwrap();

        let deserialized: CacheStats = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.hits, 5);
        assert_eq!(deserialized.misses, 3);
        assert_eq!(deserialized.evictions, 1);
        assert_eq!(deserialized.total_items_stored, 10);
        assert_eq!(deserialized.memory_usage, 1024);

        let _created_elapsed = deserialized.created_at.elapsed();
        let _last_access_elapsed = deserialized.last_access.elapsed();
    }

    #[test]
    fn test_hit_rate_calculation() {
        let mut stats = CacheStats::new();
        stats.record_hit();
        stats.record_hit();
        stats.record_miss();

        assert_eq!(stats.hit_rate(), 66.66666666666666);
    }

    #[test]
    fn test_uptime() {
        let stats = CacheStats::new();
        sleep(Duration::from_secs(1));
        assert!(stats.uptime() >= 1);
    }
}
