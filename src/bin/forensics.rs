use reqwest::Client;
use serde_json::Value;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key =
        env::var("ETHERSCAN_API_KEY").expect("Please set ETHERSCAN_API_KEY environment variable");

    let target_address = "0xdab2da14de3aba41adc3b4ede3dbc99ae28a5941";

    let url = format!(
        "https://api.etherscan.io/v2/api?chainid=56&module=account&action=tokentx&address={}&startblock=0&endblock=99999999&sort=asc&apikey={}",
        target_address, api_key
    );

    println!(
        "Scanning BSC Chain (ChainID 56) for address: {}",
        target_address
    );
    println!("URL: {}", url.replace(&api_key, "***"));

    let client = Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        panic!("HTTP Error: {}", resp.status());
    }

    let json: Value = resp.json().await?;

    if json["status"] == "1" {
        let empty_vec = Vec::new();
        let txs = json["result"].as_array().unwrap_or(&empty_vec);
        println!("Success. Found {} transactions.", txs.len());
    } else {
        println!("API Error: {}", json["message"]);
        println!("Result: {}", json["result"]);
        if json["result"].to_string().contains("deprecated") {
            println!(
                "Hint: confirm you are using an Etherscan Unified V2 API key, not a legacy BscScan key"
            );
        }
    }

    Ok(())
}
