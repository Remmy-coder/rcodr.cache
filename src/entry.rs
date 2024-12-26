use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct CacheEntry<V> {
    pub(crate) value: V,
    pub(crate) created_at: Instant,
    pub(crate) expires_at: Option<Instant>,
    pub(crate) access_count: u64,
    pub(crate) last_access: Instant,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SerializableCacheEntry<V>
where
    V: Serialize,
{
    value: V,
    created_at_duration: u64,
    expires_at_duration: Option<u64>,
    access_count: u64,
    last_access_duration: u64,
}

impl<V: Serialize> CacheEntry<V> {
    pub fn new(value: V, ttl: Option<Duration>) -> Self {
        let now = Instant::now();

        Self {
            value,
            created_at: now,
            expires_at: ttl.map(|duration| now + duration),
            access_count: 0,
            last_access: now,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map_or(false, |expires_at| expires_at <= Instant::now())
    }

    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_access = Instant::now();
    }
}

impl<V: Serialize> Serialize for CacheEntry<V> {
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

        let expires_duration = self.expires_at.map(|instant| {
            since_epoch
                .checked_sub(now_instant.duration_since(instant))
                .unwrap_or(Duration::ZERO)
        });

        let last_access_duration = since_epoch
            .checked_sub(now_instant.duration_since(self.last_access))
            .unwrap_or(Duration::ZERO);

        let serializable = SerializableCacheEntry {
            value: &self.value,
            created_at_duration: created_duration.as_secs(),
            expires_at_duration: expires_duration.map(|d| d.as_secs()),
            access_count: self.access_count,
            last_access_duration: last_access_duration.as_secs(),
        };

        serializable.serialize(serializer)
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for CacheEntry<V>
where
    V: Deserialize<'de> + Serialize,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let serializable = SerializableCacheEntry::deserialize(deserializer)?;

        let now_instant = Instant::now();
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(serde::de::Error::custom)?;

        let created_at =
            now_instant - (since_epoch - Duration::from_secs(serializable.created_at_duration));

        let expires_at = serializable
            .expires_at_duration
            .map(|secs| now_instant - (since_epoch - Duration::from_secs(secs)));

        let last_access =
            now_instant - (since_epoch - Duration::from_secs(serializable.last_access_duration));

        Ok(CacheEntry {
            value: serializable.value,
            created_at,
            expires_at,
            access_count: serializable.access_count,
            last_access,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_serialization_cache_entry() {
        let entry = CacheEntry::new("test_value", Some(Duration::from_secs(3600)));

        let serialized = serde_json::to_string(&entry).unwrap();

        let deserialized: CacheEntry<String> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.value, "test_value");
        assert_eq!(deserialized.access_count, 0);
    }

    #[test]
    fn test_expiration() {
        let entry = CacheEntry::new("test_value", Some(Duration::from_millis(100)));

        assert!(!entry.is_expired());

        std::thread::sleep(Duration::from_millis(150));
        assert!(entry.is_expired());
    }

    #[test]
    fn test_access_count() {
        let mut entry = CacheEntry::new("test_value", None);
        assert_eq!(entry.access_count, 0);

        entry.record_access();
        assert_eq!(entry.access_count, 1);
    }
}
