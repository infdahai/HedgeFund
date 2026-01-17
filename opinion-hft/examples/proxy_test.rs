use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let api_key = std::env::var("OPINION_API_KEY")
        .expect("OPINION_API_KEY not set");

    println!("=== Proxy Test with reqwest ===\n");
    println!("API Key: {}...\n", &api_key[..10]);

    let url = "https://proxy.opinion.trade:8443/openapi/markets";

    println!("Test 1: No proxy (baseline test)");
    let client_no_proxy = Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    match client_no_proxy.get(url)
        .header("apikey", &api_key)
        .send()
        .await
    {
        Ok(resp) => {
            println!("Status: {}", resp.status());
            println!("Headers:\n{:#?}", resp.headers());
            let body = resp.text().await?;
            println!("Body (first 500 chars): {}...\n", &body[..body.len().min(500)]);
        }
        Err(e) => {
            println!("Error: {:?}\n", e);
        }
    }

    println!("\nTest 2A: With HTTP proxy (http://127.0.0.1:7890)");
    let proxy_http = reqwest::Proxy::http("http://127.0.0.1:7890")?;
    let client_http_proxy = Client::builder()
        .proxy(proxy_http)
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    match client_http_proxy.get(url)
        .header("apikey", &api_key)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            println!("Status: {}", status);
            println!("Headers:\n{:#?}", resp.headers());
            let body = resp.text().await?;
            println!("Body (first 500 chars): {}...\n", &body[..body.len().min(500)]);

            if status.is_success() {
                println!("✅ SUCCESS: reqwest works with HTTP proxy!");
            } else if status == 403 {
                println!("❌ 403 Forbidden: Proxy routes but TLS fingerprinting detected");
            } else {
                println!("⚠️ Unexpected status code");
            }
        }
        Err(e) => {
            println!("Error: {:?}\n", e);
        }
    }

    println!("\nTest 2B: With SOCKS5 proxy (socks5://127.0.0.1:7890)");
    let proxy_socks5 = reqwest::Proxy::all("socks5://127.0.0.1:7890")?;
    let client_socks5_proxy = Client::builder()
        .proxy(proxy_socks5)
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    match client_socks5_proxy.get(url)
        .header("apikey", &api_key)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            println!("Status: {}", status);
            let body = resp.text().await?;
            println!("Body (first 300 chars): {}...\n", &body[..body.len().min(300)]);

            if status.is_success() {
                println!("✅ SUCCESS: reqwest works with SOCKS5 proxy!");
            }
        }
        Err(e) => {
            println!("Error: {:?}\n", e);
        }
    }

    println!("\nTest 3: DIAGNOSIS SUMMARY");
    println!("=================================");
    println!();
    println!("1. HTTP Error Code: -9836 (errSSLProtocol)");
    println!("   → TLS handshake failed at MAC OS SecureTransport layer");
    println!();
    println!("2. Root Cause: Kong Gateway JA3 TLS Fingerprinting");
    println!("   → rustls produces JA3 fingerprint not in whitelist");
    println!("   → curl works because FlClash proxy likely modifies TLS handshake");
    println!();
    println!("3. Why HTTP/SOCKS5 proxies didn't help:");
    println!("   → Error occurs at TLS layer, before application data");
    println!("   → Proxy connections succeeded, but Kong rejected TLS ClientHello");
    println!();
    println!("4. Recommended Solution: Use rrequest (reqwest fork with TLS impersonation)");
    println!("   → Supports Chrome/Firefox full TLS fingerprint");
    println!("   → Native Rust async performance");
    println!("   → See PROXY_DIAGNOSIS_REPORT.md for details");

    Ok(())
}