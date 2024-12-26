use std::time::Duration;

use cache::Cache;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cache: Cache<String, String> = Cache::<String, String>::builder()
        .max_size(5)
        .default_ttl(Duration::from_secs(60))
        .persistent_path("cache.json")
        .build_async()
        .await?;

    cache.put("key1".to_string(), "value1".to_string(), None)?;
    println!("Get key1: {:?}", cache.get(&"key1".to_string())?);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let contents = tokio::fs::read_to_string("cache.json").await?;
    println!("Cache file contents: {}", contents);

    let cache2: Cache<String, String> = Cache::<String, String>::builder()
        .persistent_path("cache.json")
        .build_async()
        .await?;

    println!(
        "Retrieved after restart: {:?}",
        cache2.get(&"key1".to_string())?
    );

    println!("Cache stats: {:?}", cache2.stats()?);

    Ok(())
}
