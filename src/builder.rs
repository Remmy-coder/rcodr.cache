use std::{hash::Hash, time::Duration};

use serde::Serialize;

use crate::{
    storage::{FileStorage, Storage},
    Cache, CacheError,
};

pub struct CacheBuilder {
    max_size: Option<usize>,
    default_ttl: Option<Duration>,
    persistent_path: Option<String>,
    eviction_policy: EvictionPolicy,
}

#[derive(Clone, Copy, Debug)]
pub enum EvictionPolicy {
    LRU,
    FIFO,
    LFU,
}

impl Default for CacheBuilder {
    fn default() -> Self {
        Self {
            max_size: None,
            default_ttl: None,
            persistent_path: None,
            eviction_policy: EvictionPolicy::LRU,
        }
    }
}

impl CacheBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_size(mut self, size: usize) -> Self {
        self.max_size = Some(size);
        self
    }

    pub fn default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = Some(ttl);
        self
    }

    pub fn persistent_path(mut self, path: impl Into<String>) -> Self {
        self.persistent_path = Some(path.into());
        self
    }

    pub fn eviction_policy(mut self, policy: EvictionPolicy) -> Self {
        self.eviction_policy = policy;
        self
    }

    pub async fn build_async<K, V>(&self) -> Result<Cache<K, V>, CacheError>
    where
        K: Eq + Hash + Clone + Send + Sync + Serialize + serde::de::DeserializeOwned + 'static,
        V: Clone + Send + Sync + Serialize + serde::de::DeserializeOwned + 'static,
    {
        let storage = if let Some(path) = &self.persistent_path {
            Some(Box::new(FileStorage::new(path))
                as Box<dyn Storage<Key = K, Value = V> + Send + Sync>)
        } else {
            None
        };

        Cache::new_async(
            self.max_size,
            self.default_ttl,
            storage,
            self.eviction_policy,
        )
        .await
    }

    pub fn build<K, V>(&self) -> Result<Cache<K, V>, CacheError>
    where
        K: Eq + Hash + Clone + Send + Sync + Serialize + serde::de::DeserializeOwned + 'static,
        V: Clone + Send + Sync + Serialize + serde::de::DeserializeOwned + 'static,
    {
        if self.persistent_path.is_some() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| CacheError::PersistenceError(e.to_string()))?;
            runtime.block_on(self.build_async())
        } else {
            Cache::new(self.max_size, self.default_ttl, None, self.eviction_policy)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::CacheEntry;

    use super::*;
    use serde::{Deserialize, Serialize};
    use std::{fs::File, io::Write};
    use tempfile::NamedTempFile;

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct TestKey(String);

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestValue(String);

    #[test]
    fn test_builder_default() {
        let builder = CacheBuilder::new();
        assert!(builder.max_size.is_none());
        assert!(builder.default_ttl.is_none());
        assert!(builder.persistent_path.is_none());
        assert!(matches!(builder.eviction_policy, EvictionPolicy::LRU));
    }

    #[test]
    fn test_builder_with_max_size() {
        let builder = CacheBuilder::new().max_size(100);
        assert_eq!(builder.max_size, Some(100));
    }

    #[test]
    fn test_builder_with_ttl() {
        let ttl = Duration::from_secs(60);
        let builder = CacheBuilder::new().default_ttl(ttl);
        assert_eq!(builder.default_ttl, Some(ttl));
    }

    #[test]
    fn test_builder_with_persistent_path() {
        let path = "test_cache.json".to_string();
        let builder = CacheBuilder::new().persistent_path(path.clone());
        assert_eq!(builder.persistent_path, Some(path));
    }

    #[test]
    fn test_builder_with_eviction_policy() {
        let builder = CacheBuilder::new().eviction_policy(EvictionPolicy::FIFO);
        assert!(matches!(builder.eviction_policy, EvictionPolicy::FIFO));
    }

    #[test]
    fn test_builder_chaining() {
        let ttl = Duration::from_secs(60);
        let builder = CacheBuilder::new()
            .max_size(100)
            .default_ttl(ttl)
            .eviction_policy(EvictionPolicy::LFU);

        assert_eq!(builder.max_size, Some(100));
        assert_eq!(builder.default_ttl, Some(ttl));
        assert!(matches!(builder.eviction_policy, EvictionPolicy::LFU));
    }

    #[test]
    fn test_build_in_memory_cache() {
        let cache: Result<Cache<TestKey, TestValue>, _> = CacheBuilder::new().max_size(10).build();

        assert!(cache.is_ok());
        let cache = cache.unwrap();

        for i in 0..5 {
            let key = TestKey(format!("key{}", i));
            let value = TestValue(format!("value{}", i));
            cache
                .store()
                .write()
                .unwrap()
                .insert(key, CacheEntry::new(value, None));
        }

        assert_eq!(cache.store().read().unwrap().len(), 5);
    }

    #[test]
    fn test_build_persistent_cache() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap().to_string();

        {
            let mut file = File::create(&temp_path).unwrap();
            file.write_all(b"[]").unwrap();
        }

        let cache: Result<Cache<TestKey, TestValue>, _> =
            CacheBuilder::new().persistent_path(temp_path).build();

        assert!(cache.is_ok());
    }

    #[test]
    fn test_build_with_invalid_path() {
        let result: Result<Cache<TestKey, TestValue>, _> = CacheBuilder::new()
            .persistent_path("/invalid/path/that/does/not/exist/cache.json")
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_build_with_ttl() {
        let cache: Result<Cache<TestKey, TestValue>, _> = CacheBuilder::new()
            .default_ttl(Duration::from_secs(60))
            .build();

        assert!(cache.is_ok());
        let cache = cache.unwrap();

        let key = TestKey("test".to_string());
        let value = TestValue("value".to_string());
        cache
            .store()
            .write()
            .unwrap()
            .insert(key, CacheEntry::new(value, Some(Duration::from_secs(60))));
    }
}
