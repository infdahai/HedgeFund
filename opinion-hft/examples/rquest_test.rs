//! rquest integration test for Opinion API

use rquest::Client;
use rquest_util::Emulation;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== rquest Chrome 120 TLS Impersonation Test ===\n");

    let api_key = std::env::var("OPINION_API_KEY")
        .unwrap_or_else(|_| "KVzgNu47iggXrCsnV1ZlVAV0hmBTN23U".to_string());

    println!("Building rrequest client with Chrome 120 emulation...");
    let client = Client::builder()
        .emulation(Emulation::Chrome120)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    println!("✓ Client built successfully\n");

    let base_url = "https://proxy.opinion.trade:8443/openapi";

    println!("Test 1: GET /market");
    println!("URL: {}/market", base_url);
    let start = Instant::now();
    let resp = client
        .get(&format!("{}/market", base_url))
        .header("apikey", &api_key)
        .header("Accept", "application/json")
        .send()
        .await?;
    let elapsed = start.elapsed();
    println!("Status: {}", resp.status());
    println!("Latency: {:?}", elapsed);
    
    if resp.status().is_success() {
        let text = resp.text().await?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(data) = json.get("result") {
                if let Some(list) = data.get("list") {
                    if let Some(arr) = list.as_array() {
                        println!("✓ Successfully parsed {} markets", arr.len());
                    }
                }
            }
        }
    } else {
        let text = resp.text().await?;
        println!("✗ Response: {}", text);
    }
    println!();

    println!("Test 2: GET /quoteToken?chainId=56");
    println!("URL: {}/quoteToken?chainId=56", base_url);
    let start = Instant::now();
    let resp = client
        .get(&format!("{}/quoteToken?chainId=56", base_url))
        .header("apikey", &api_key)
        .header("Accept", "application/json")
        .send()
        .await?;
    let elapsed = start.elapsed();
    println!("Status: {}", resp.status());
    println!("Latency: {:?}", elapsed);

    if resp.status().is_success() {
        let text = resp.text().await?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(data) = json.get("result") {
                if let Some(list) = data.get("list") {
                    if let Some(arr) = list.as_array() {
                        println!("✓ Successfully parsed {} quote tokens", arr.len());
                        for token in arr.iter().take(1) {
                            println!("  Token: {}", serde_json::to_string_pretty(token)?);
                        }
                    }
                }
            }
        }
    } else {
        let text = resp.text().await?;
        println!("✗ Response: {}", text);
    }
    println!();

    println!("Test 3: Verify TLS fingerprint");
    println!("URL: https://tls.peet.ws/api/all");
    let start = Instant::now();
    let resp = client
        .get("https://tls.peet.ws/api/all")
        .send()
        .await?;
    let elapsed = start.elapsed();
    println!("Status: {}", resp.status());
    println!("Latency: {:?}", elapsed);

    if resp.status().is_success() {
        let text = resp.text().await?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(ja3) = json.get("ja3") {
                println!("✓ JA3 Hash: {}", ja3);
            }
            if let Some(ja4) = json.get("ja4_hash") {
                println!("✓ JA4 Hash: {}", ja4);
            }
        }
    }

    println!("\n=== All Tests Complete ===");
    Ok(())
}
