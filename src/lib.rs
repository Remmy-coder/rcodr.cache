pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

mod builder;
mod entry;
mod error;
mod stats;
mod storage;
mod sync_cache;

pub use entry::CacheEntry;
pub use error::CacheError;
pub use stats::CacheStats;
pub use sync_cache::Cache;
