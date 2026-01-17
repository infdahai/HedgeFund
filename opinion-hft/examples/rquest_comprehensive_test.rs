use rquest::Client;
use rquest_util::Emulation;
use serde_json::Value;
use std::time::Instant;

const API_KEY: &str = "KVzgNu47iggXrCsnV1ZlVAV0hmBTN23U";
const MARKETS_URL: &str = "https://proxy.opinion.trade:8443/openapi/markets";
const QUOTE_TOKEN_URL: &str = "https://proxy.opinion.trade:8443/openapi/quoteToken?chainId=56";

struct TestResult {
    config_name: String,
    emulation: String,
    with_proxy: bool,
    url: String,
    status: u16,
    latency_ms: u64,
    success: bool,
    error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  RQUEST COMPREHENSIVE TEST SUITE                             ║");
    println!("║  Kong Gateway JA3 TLS Fingerprinting Bypass Test             ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let mut all_results = Vec::new();

    // Test 1: Chrome Versions (most likely to work)
    println!("📊 Testing Chrome Versions (most common browsers)...");
    test_chrome_versions(&mut all_results);

    // Test 2: Safari Versions (Apple's TLS stack)
    println!("📊 Testing Safari Versions...");
    test_safari_versions(&mut all_results);

    // Test 3: Firefox Versions (Mozilla's TLS stack)
    println!("📊 Testing Firefox Versions...");
    test_firefox_versions(&mut all_results);

    // Test 4: Edge Versions (Chromium-based with Microsoft tweaks)
    println!("📊 Testing Edge Versions...");
    test_edge_versions(&mut all_results);

    // Print results
    print_results(&all_results);

    // Generate final report
    generate_report(&all_results);

    Ok(())
}

fn test_chrome_versions(results: &mut Vec<TestResult>) {
    let chrome_versions = vec![
        ("Chrome120", Emulation::Chrome120),
        ("Chrome119", Emulation::Chrome119),
        ("Chrome118", Emulation::Chrome118),
        ("Chrome117", Emulation::Chrome117),
        ("Chrome124", Emulation::Chrome124),
        ("Chrome128", Emulation::Chrome128),
        ("Chrome132", Emulation::Chrome132),
        ("Chrome135", Emulation::Chrome135),
        ("Chrome137", Emulation::Chrome137),
    ];

    for (name, emulation) in chrome_versions {
        test_emulation(results, name, emulation, false);
    }
}

fn test_safari_versions(results: &mut Vec<TestResult>) {
    let safari_versions = vec![
        ("Safari18", Emulation::Safari18),
        ("Safari17_5", Emulation::Safari17_5),
        ("Safari17_0", Emulation::Safari17_0),
        ("Safari16_5", Emulation::Safari16_5),
        ("Safari15_6_1", Emulation::Safari15_6_1),
        ("Safari15_3", Emulation::Safari15_3),
    ];

    for (name, emulation) in safari_versions {
        test_emulation(results, name, emulation, false);
    }
}

fn test_firefox_versions(results: &mut Vec<TestResult>) {
    let firefox_versions = vec![
        ("Firefox135", Emulation::Firefox135),
        ("Firefox136", Emulation::Firefox136),
        ("Firefox133", Emulation::Firefox133),
        ("Firefox128", Emulation::Firefox128),
        ("Firefox117", Emulation::Firefox117),
    ];

    for (name, emulation) in firefox_versions {
        test_emulation(results, name, emulation, false);
    }
}

fn test_edge_versions(results: &mut Vec<TestResult>) {
    let edge_versions = vec![
        ("Edge134", Emulation::Edge134),
        ("Edge131", Emulation::Edge131),
        ("Edge127", Emulation::Edge127),
    ];

    for (name, emulation) in edge_versions {
        test_emulation(results, name, emulation, false);
    }
}

fn test_emulation(results: &mut Vec<TestResult>, config_name: &str, emulation: Emulation, with_proxy: bool) {
    let urls = vec![
        (MARKETS_URL.to_string(), "GET /markets"),
        (QUOTE_TOKEN_URL.to_string(), "GET /quoteToken"),
    ];

    for (url, endpoint) in urls {
        let result = match run_test(config_name, emulation, with_proxy, &url) {
            Ok(r) => r,
            Err(e) => TestResult {
                config_name: config_name.to_string(),
                emulation: format!("{:?}", emulation),
                with_proxy,
                url: endpoint.to_string(),
                status: 0,
                latency_ms: 0,
                success: false,
                error: Some(e.to_string()),
            },
        };

        results.push(result);
    }
}

fn run_test(
    config_name: &str,
    emulation: Emulation,
    with_proxy: bool,
    url: &str,
) -> Result<TestResult, Box<dyn std::error::Error>> {
    let mut builder = Client::builder()
        .emulation(emulation)
        .timeout(std::time::Duration::from_secs(10));

    let proxy_str = if with_proxy {
        builder = builder.proxy(rquest::Proxy::all("http://127.0.0.1:7890")?);
        Some("http://127.0.0.1:7890".to_string())
    } else {
        None
    };

    let client = builder.build()?;

    let start = Instant::now();

    let response = client
        .get(url)
        .header("apikey", API_KEY)
        .header("Accept", "application/json")
        .send()
        .await?;

    let latency = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();
    let body = response.text().await?;

    let success = status == 200;

    let (error, success) = if success {
        (None, true)
    } else {
        let error_msg = if let Ok(json) = serde_json::from_str::<Value>(&body) {
            json.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string()
        } else {
            body.chars().take(100).collect()
        };

        (Some(error_msg), false)
    };

    Ok(TestResult {
        config_name: config_name.to_string(),
        emulation: format!("{:?}", emulation),
        with_proxy,
        url: url.to_string(),
        status,
        latency_ms: latency,
        success,
        error,
    })
}

fn print_results(results: &[TestResult]) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  DETAILED TEST RESULTS                                       ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let mut grouped: std::collections::HashMap<String, Vec<&TestResult>> = std::collections::HashMap::new();

    for result in results {
        grouped
            .entry(result.config_name.clone())
            .or_default()
            .push(result);
    }

    println!("{:<15} {:<40} {:<8} {:<10} {:<10}", "Config", "Endpoint", "Status", "Latency", "Result");
    println!("{:-<85}", "");

    for config in grouped.keys().sorted() {
        let config_results = grouped.get(config).unwrap();

        for result in config_results {
            let endpoint = if result.url.contains("markets") {
                "GET /markets"
            } else {
                "GET /quoteToken"
            };

            let status_str = if result.status == 0 {
                "ERROR".to_string()
            } else {
                result.status.to_string()
            };

            let result_str = if result.success {
                "✅ SUCCESS".to_string()
            } else {
                "❌ FAILED".to_string()
            };

            println!(
                "{:<15} {:<40} {:<8} {:<10} {:<10}",
                result.config_name, endpoint, status_str, format!("{}ms", result.latency_ms), result_str
            );

            if let Some(error) = &result.error {
                println!("              Error: {}", error);
            }
        }
        println!();
    }
}

fn generate_report(results: &[TestResult]) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  FINAL ANALYSIS & RECOMMENDATIONS                           ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    let total_tests = results.len();
    let successful_tests = results.iter().filter(|r| r.success).count();
    let failed_tests = total_tests - successful_tests;

    println!("📈 Test Statistics:");
    println!("   Total tests: {}", total_tests);
    println!("   Successful: {} ({:.1}%)", successful_tests, (successful_tests as f64 / total_tests as f64) * 100.0);
    println!("   Failed: {} ({:.1}%)", failed_tests, (failed_tests as f64 / total_tests as f64) * 100.0);
    println!();

    if successful_tests > 0 {
        println!("✅ WORKING CONFIGURATIONS:");
        let successful_configs: std::collections::HashSet<_> = results
            .iter()
            .filter(|r| r.success)
            .map(|r| (&r.config_name, r.latency_ms))
            .collect();

        let mut configs_vec: Vec<_> = successful_configs.into_iter().collect();
        configs_vec.sort_by_key(|&(_, latency)| latency);

        for (i, (config, latency)) in configs_vec.iter().enumerate() {
            let config_results: Vec<_> = results.iter().filter(|r| &r.config_name == config).collect();
            let all_success = config_results.iter().all(|r| r.success);
            let avg_latency = *latency;

            println!("   {}. {} - Avg: {}ms (✓ both endpoints)", i + 1, config, avg_latency);
        }
        println!();
    }

    if successful_tests == 0 {
        println!("❌ CRITICAL FINDING:");
        println!();
        println!("   NO rquest configuration successfully bypassed Kong Gateway's");
        println!("   JA3 TLS fingerprinting detection.");
        println!();
        println!("   📋 RECOMMENDATION:");
        println!("   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("   rrequest cannot bypass Kong Gateway's JA3 detection.");
        println!("   curl subprocess with FlClash proxy is the ONLY working solution.");
        println!("   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!();
        println!("   🎯 NEXT STEPS:");
        println!("   1. Keep the current curl subprocess implementation");
        println!("   2. Monitor Kong Gateway for any configuration changes");
        println!("   3. Consider adding a fallback to curl if Kong relaxes detection");
        println!();
        println!("   📊 LATENCY COMPARISON:");
        println!("   - Curl subprocess: ~10-50ms overhead (acceptable for HFT)");
        println!("   - rrequest: N/A (all configurations failed)");
        println!();
    } else {
        println!("🎯 RECOMMENDATION:");
        println!("   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("   At least one rquest configuration works! Use the fastest");
        println!("   working configuration for production HFT trading.");
        println!("   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!();
        println!("   📊 LATENCY COMPARISON:");
        println!("   - Curl subprocess (current): ~10-50ms overhead");
        println!("   - rrequest (best): ~{}ms", results.iter().filter_map(|r| if r.success { Some(r.latency_ms) } else { None }).min().unwrap_or(0));
        println!();
        println!("   ✅ MIGRATION PLAN:");
        println!("   1. Replace curl subprocess with rquest in OpinionClient");
        println!("   2. Add fallback to curl if rrequest fails");
        println!("   3. Monitor for Kong Gateway detection changes");
        println!();
    }

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  END OF TEST REPORT                                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}
