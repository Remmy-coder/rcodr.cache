use std::{marker::PhantomData, path::Path};

use serde::{de::DeserializeOwned, Serialize};

use crate::CacheError;

#[async_trait::async_trait]
pub trait Storage {
    type Key;
    type Value;

    async fn save(&self, data: &[(Self::Key, Self::Value)]) -> Result<(), CacheError>;

    async fn load(&self) -> Result<Vec<(Self::Key, Self::Value)>, CacheError>;

    fn clone_box(&self) -> Box<dyn Storage<Key = Self::Key, Value = Self::Value> + Send + Sync>;
}

#[derive(Clone)]
pub struct FileStorage<K, V> {
    path: String,
    _phantom: PhantomData<(K, V)>,
}

impl<K, V> FileStorage<K, V> {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            _phantom: PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<K, V> Storage for FileStorage<K, V>
where
    K: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    V: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
{
    type Key = K;
    type Value = V;

    async fn save(&self, data: &[(Self::Key, Self::Value)]) -> Result<(), CacheError> {
        let serialized = serde_json::to_string(&data)
            .map_err(|e| CacheError::SerializationError(e.to_string()))?;

        tokio::fs::write(&self.path, serialized)
            .await
            .map_err(|e| CacheError::PersistenceError(e.to_string()))?;

        Ok(())
    }

    async fn load(&self) -> Result<Vec<(Self::Key, Self::Value)>, CacheError> {
        if !Path::new(&self.path).exists() {
            return Ok(Vec::new());
        }

        let data = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(|e| CacheError::PersistenceError(e.to_string()))?;

        serde_json::from_str(&data).map_err(|e| CacheError::SerializationError(e.to_string()))
    }

    fn clone_box(&self) -> Box<dyn Storage<Key = K, Value = V> + Send + Sync> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::NamedTempFile;

    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    struct TestStruct {
        id: i32,
        name: String,
    }

    #[tokio::test]
    async fn test_file_storage_with_custom_type() -> Result<(), CacheError> {
        let temp_file = NamedTempFile::new().unwrap();
        let storage: FileStorage<i32, TestStruct> =
            FileStorage::new(temp_file.path().to_str().unwrap());

        let test_data = vec![
            (
                1,
                TestStruct {
                    id: 1,
                    name: "test1".to_string(),
                },
            ),
            (
                2,
                TestStruct {
                    id: 2,
                    name: "test2".to_string(),
                },
            ),
        ];

        storage.save(&test_data).await?;
        let loaded_data = storage.load().await?;
        assert_eq!(test_data, loaded_data);
        Ok(())
    }
}
