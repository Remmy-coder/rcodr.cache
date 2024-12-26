use serde::Serialize;
use std::{collections::HashMap, hash::Hash, sync::RwLock, time::Duration};

use crate::{
    builder::{CacheBuilder, EvictionPolicy},
    storage::Storage,
    CacheEntry, CacheError, CacheStats,
};

pub struct Cache<K, V> {
    store: RwLock<HashMap<K, CacheEntry<V>>>,
    max_size: Option<usize>,
    default_ttl: Option<Duration>,
    stats: RwLock<CacheStats>,
    eviction_policy: EvictionPolicy,
    storage: Option<Box<dyn Storage<Key = K, Value = V> + Send + Sync>>,
}

impl<K, V> Cache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + Serialize + serde::de::DeserializeOwned + 'static,
    V: Clone + Send + Sync + Serialize + serde::de::DeserializeOwned + 'static,
{
    pub(crate) async fn new_async(
        max_size: Option<usize>,
        default_ttl: Option<Duration>,
        storage: Option<Box<dyn Storage<Key = K, Value = V> + Send + Sync>>,
        eviction_policy: EvictionPolicy,
    ) -> Result<Self, CacheError> {
        let stats = CacheStats::new();

        let mut cache = Self {
            store: RwLock::new(HashMap::new()),
            max_size,
            default_ttl,
            stats: RwLock::new(stats),
            eviction_policy,
            storage: None,
        };

        if let Some(storage) = storage {
            let items = storage
                .load()
                .await
                .map_err(|e| CacheError::PersistenceError(e.to_string()))?;

            {
                let mut store = cache.store.write().map_err(|_| CacheError::LockError)?;
                for (key, value) in items {
                    store.insert(key, CacheEntry::new(value, default_ttl));
                }
            }

            cache.storage = Some(storage);
        }

        Ok(cache)
    }

    pub(crate) fn new(
        max_size: Option<usize>,
        default_ttl: Option<Duration>,
        storage: Option<Box<dyn Storage<Key = K, Value = V> + Send + Sync>>,
        eviction_policy: EvictionPolicy,
    ) -> Result<Self, CacheError> {
        if storage.is_some() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| CacheError::PersistenceError(e.to_string()))?;
            runtime.block_on(Self::new_async(
                max_size,
                default_ttl,
                storage,
                eviction_policy,
            ))
        } else {
            Ok(Self {
                store: RwLock::new(HashMap::new()),
                max_size,
                default_ttl,
                stats: RwLock::new(CacheStats::new()),
                eviction_policy,
                storage: None,
            })
        }
    }

    pub fn builder() -> CacheBuilder {
        CacheBuilder::new()
    }

    pub fn put(&self, key: K, value: V, ttl: Option<Duration>) -> Result<Option<V>, CacheError> {
        self.remove_expired()?;

        let (old_value, items_to_save) = {
            let mut store = self.store.write().map_err(|_| CacheError::LockError)?;
            let mut stats = self.stats.write().map_err(|_| CacheError::LockError)?;

            if let Some(max_size) = self.max_size {
                while store.len() >= max_size {
                    match self.eviction_policy {
                        EvictionPolicy::LRU => self.evict_lru(&mut store)?,
                        EvictionPolicy::FIFO => self.evict_fifo(&mut store)?,
                        EvictionPolicy::LFU => self.evict_lfu(&mut store)?,
                    }
                    stats.record_eviction();
                }
            }

            let entry = CacheEntry::new(value.clone(), ttl.or(self.default_ttl));
            let old_value = store.insert(key.clone(), entry).map(|old| old.value);
            stats.total_items_stored += 1;

            let items_to_save = vec![(key, value)];
            (old_value, items_to_save)
        };

        if let Some(storage) = &self.storage {
            let storage = storage.clone_box();

            // Spawn task to handle storage
            let save_future = async move {
                if let Err(e) = storage.save(&items_to_save).await {
                    eprintln!("Error saving to storage: {:?}", e);
                }
            };

            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.spawn(save_future);
                }
                Err(_) => {
                    let rt = tokio::runtime::Runtime::new()
                        .map_err(|e| CacheError::PersistenceError(e.to_string()))?;
                    rt.block_on(save_future);
                }
            }
        }

        Ok(old_value)
    }

    pub fn get(&self, key: &K) -> Result<Option<V>, CacheError> {
        self.remove_expired()?;

        let mut store = self.store.write().map_err(|_| CacheError::LockError)?;
        let mut stats = self.stats.write().map_err(|_| CacheError::LockError)?;

        match store.get_mut(key) {
            Some(entry) => {
                entry.record_access();
                stats.record_hit();
                Ok(Some(entry.value.clone()))
            }
            None => {
                stats.record_miss();
                Ok(None)
            }
        }
    }

    pub fn remove(&self, key: &K) -> Result<Option<V>, CacheError> {
        let mut store = self.store.write().map_err(|_| CacheError::LockError)?;
        Ok(store.remove(key).map(|entry| entry.value))
    }

    pub fn clear(&self) -> Result<(), CacheError> {
        let mut store = self.store.write().map_err(|_| CacheError::LockError)?;
        store.clear();
        Ok(())
    }

    pub fn stats(&self) -> Result<CacheStats, CacheError> {
        let stats = self.stats.read().map_err(|_| CacheError::LockError)?;
        Ok(stats.clone())
    }

    pub fn len(&self) -> Result<usize, CacheError> {
        let store = self.store.read().map_err(|_| CacheError::LockError)?;
        Ok(store.len())
    }

    pub fn is_empty(&self) -> Result<bool, CacheError> {
        Ok(self.len()? == 0)
    }

    fn remove_expired(&self) -> Result<(), CacheError> {
        let mut store = self.store.write().map_err(|_| CacheError::LockError)?;
        store.retain(|_, entry| !entry.is_expired());
        Ok(())
    }

    fn evict_lru(&self, store: &mut HashMap<K, CacheEntry<V>>) -> Result<(), CacheError> {
        if let Some((key, _)) = store
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(k, _)| (k.clone(), ()))
        {
            store.remove(&key);
        }

        Ok(())
    }

    fn evict_fifo(&self, store: &mut HashMap<K, CacheEntry<V>>) -> Result<(), CacheError> {
        if let Some((key, _)) = store
            .iter()
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(k, _)| (k.clone(), ()))
        {
            store.remove(&key);
        }

        Ok(())
    }

    fn evict_lfu(&self, store: &mut HashMap<K, CacheEntry<V>>) -> Result<(), CacheError> {
        if let Some((key, _)) = store
            .iter()
            .min_by_key(|(_, entry)| entry.access_count)
            .map(|(k, _)| (k.clone(), ()))
        {
            store.remove(&key);
        }

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn store(&self) -> &RwLock<HashMap<K, CacheEntry<V>>> {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct TestKey(String);

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestValue(String);

    fn create_test_cache(
        policy: EvictionPolicy,
        max_size: Option<usize>,
    ) -> Cache<TestKey, TestValue> {
        CacheBuilder::new()
            .max_size(max_size.unwrap_or(10))
            .eviction_policy(policy)
            .build()
            .unwrap()
    }

    #[test]
    fn test_basic_operations() {
        let cache = create_test_cache(EvictionPolicy::LRU, None);

        let key = TestKey("key1".to_string());
        let value = TestValue("value1".to_string());
        assert!(cache
            .put(key.clone(), value.clone(), None)
            .unwrap()
            .is_none());

        assert_eq!(cache.get(&key).unwrap(), Some(value.clone()));

        assert_eq!(cache.remove(&key).unwrap(), Some(value));
        assert!(cache.get(&key).unwrap().is_none());
    }

    #[test]
    fn test_max_size_eviction_lru() {
        let cache = create_test_cache(EvictionPolicy::LRU, Some(2));

        cache
            .put(
                TestKey("key1".to_string()),
                TestValue("value1".to_string()),
                None,
            )
            .unwrap();
        cache
            .put(
                TestKey("key2".to_string()),
                TestValue("value2".to_string()),
                None,
            )
            .unwrap();

        cache.get(&TestKey("key1".to_string())).unwrap();

        cache
            .put(
                TestKey("key3".to_string()),
                TestValue("value3".to_string()),
                None,
            )
            .unwrap();

        assert!(cache.get(&TestKey("key1".to_string())).unwrap().is_some());
        assert!(cache.get(&TestKey("key2".to_string())).unwrap().is_none());
        assert!(cache.get(&TestKey("key3".to_string())).unwrap().is_some());
    }

    #[test]
    fn test_ttl() {
        let cache = create_test_cache(EvictionPolicy::LRU, None);
        let key = TestKey("key1".to_string());
        let value = TestValue("value1".to_string());

        cache
            .put(key.clone(), value, Some(Duration::from_millis(1)))
            .unwrap();

        assert!(cache.get(&key).unwrap().is_some());

        std::thread::sleep(Duration::from_millis(2));

        assert!(cache.get(&key).unwrap().is_none());
    }

    #[test]
    fn test_stats() {
        let cache = create_test_cache(EvictionPolicy::LRU, None);
        let key = TestKey("key1".to_string());

        cache.get(&key).unwrap();
        let stats = cache.stats().unwrap();
        assert_eq!(stats.misses, 1);

        cache
            .put(key.clone(), TestValue("value1".to_string()), None)
            .unwrap();
        cache.get(&key).unwrap();
        let stats = cache.stats().unwrap();
        assert_eq!(stats.hits, 1);
    }

    #[test]
    fn test_clear() {
        let cache = create_test_cache(EvictionPolicy::LRU, None);

        cache
            .put(
                TestKey("key1".to_string()),
                TestValue("value1".to_string()),
                None,
            )
            .unwrap();
        cache
            .put(
                TestKey("key2".to_string()),
                TestValue("value2".to_string()),
                None,
            )
            .unwrap();

        assert_eq!(cache.len().unwrap(), 2);
        cache.clear().unwrap();
        assert!(cache.is_empty().unwrap());
    }

    #[tokio::test]
    async fn test_persistence() -> Result<(), CacheError> {
        use std::fs::File;
        use std::io::Write;

        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        {
            let mut file = File::create(temp_path).unwrap();
            file.write_all(b"[]").unwrap();
            file.flush().unwrap();
        }

        let cache: Cache<TestKey, TestValue> = CacheBuilder::new()
            .persistent_path(temp_path)
            .build_async()
            .await?;

        let key = TestKey("persist_key".to_string());
        let value = TestValue("persist_value".to_string());
        cache.put(key.clone(), value.clone(), None)?;

        if let Some(storage) = &cache.storage {
            storage.save(&[(key.clone(), value.clone())]).await?;
        }

        let new_cache: Cache<TestKey, TestValue> = CacheBuilder::new()
            .persistent_path(temp_path)
            .build_async()
            .await?;

        assert_eq!(new_cache.get(&key)?, Some(value));
        Ok(())
    }
}
