use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use csv::Writer;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;

const TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const DEFAULT_BLOCK_RANGE: u64 = 150_000;

#[derive(Debug, Deserialize)]
struct RpcLog {
    address: String,
    #[serde(rename = "blockNumber")]
    block_number: String,
    #[serde(rename = "transactionHash")]
    transaction_hash: String,
    #[serde(rename = "logIndex")]
    log_index: String,
    data: String,
    topics: Vec<String>,
}

#[derive(Debug, Default)]
struct OBIStats {
    sum: f64,
    count: u64,
}

#[derive(Debug)]
struct ForensicsRow {
    tx_hash: String,
    block_number: u64,
    timestamp: i64,
    direction: String,
    token_contract: String,
    value_raw: String,
    log_index: String,
    from: String,
    to: String,
    obi_estimate: Option<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let (target_address, output_path, features_path, start_block, end_block, mut chunk_size) =
        parse_args()?;
    let rpc_url = env::var("BSC_RPC_URL")
        .or_else(|_| env::var("OPINION_RPC_URL_PRIMARY"))
        .context("BSC_RPC_URL or OPINION_RPC_URL_PRIMARY is not set")?;

    let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
    let (latest_block, latest_ts) = fetch_latest_block_info(&client, &rpc_url).await?;
    let start_block =
        start_block.unwrap_or_else(|| latest_block.saturating_sub(DEFAULT_BLOCK_RANGE));
    let end_block = end_block.unwrap_or(latest_block);

    let features = if let Some(path) = features_path {
        Some(load_features(&path)?)
    } else {
        None
    };

    println!(
        "Scanning blocks {} to {} (chunk {}).",
        start_block, end_block, chunk_size
    );

    let mut rows = Vec::new();
    let target_topic = pad_topic_address(&target_address)?;

    let mut current = start_block;
    while current <= end_block {
        let logs_in: Vec<RpcLog>;
        let logs_out: Vec<RpcLog>;
        loop {
            let to_block = (current + chunk_size - 1).min(end_block);
            match fetch_logs(
                &client,
                &rpc_url,
                current,
                to_block,
                None,
                Some(&target_topic),
            )
            .await
            {
                Ok(result) => {
                    let incoming = result;
                    match fetch_logs(
                        &client,
                        &rpc_url,
                        current,
                        to_block,
                        Some(&target_topic),
                        None,
                    )
                    .await
                    {
                        Ok(outgoing) => {
                            logs_in = incoming;
                            logs_out = outgoing;
                            current = to_block.saturating_add(1);
                            break;
                        }
                        Err(err) if is_rate_limited(&err) => {
                            chunk_size = shrink_chunk(chunk_size);
                            sleep(Duration::from_millis(250)).await;
                            continue;
                        }
                        Err(err) => return Err(err),
                    }
                }
                Err(err) if is_rate_limited(&err) => {
                    chunk_size = shrink_chunk(chunk_size);
                    sleep(Duration::from_millis(250)).await;
                    continue;
                }
                Err(err) => return Err(err),
            }
        }

        rows.extend(
            process_logs(
                &logs_in,
                latest_block,
                latest_ts,
                &target_address,
                "BUY/IN",
                features.as_ref(),
            )
            .await?,
        );

        rows.extend(
            process_logs(
                &logs_out,
                latest_block,
                latest_ts,
                &target_address,
                "SELL/OUT",
                features.as_ref(),
            )
            .await?,
        );

        sleep(Duration::from_millis(120)).await;
    }

    write_report(&output_path, &rows)?;
    println!("Report written: {}", output_path.display());
    Ok(())
}

#[allow(clippy::type_complexity)]
fn parse_args() -> Result<(
    String,
    std::path::PathBuf,
    Option<std::path::PathBuf>,
    Option<u64>,
    Option<u64>,
    u64,
)> {
    let mut address = "0xdab2da14de3aba41adc3b4ede3dbc99ae28a5941".to_string();
    let mut output = "reports/forensics_dab2da.csv".to_string();
    let mut features = None;
    let mut start_block = None;
    let mut end_block = None;
    let mut chunk_size = 500u64;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--address" => {
                address = args.next().context("--address requires a value")?;
            }
            "--output" => {
                output = args.next().context("--output requires a value")?;
            }
            "--features" => {
                features = Some(args.next().context("--features requires a value")?);
            }
            "--start-block" => {
                start_block = Some(
                    args.next()
                        .context("--start-block requires a value")?
                        .parse()
                        .context("invalid --start-block")?,
                );
            }
            "--end-block" => {
                end_block = Some(
                    args.next()
                        .context("--end-block requires a value")?
                        .parse()
                        .context("invalid --end-block")?,
                );
            }
            "--chunk-size" => {
                chunk_size = args
                    .next()
                    .context("--chunk-size requires a value")?
                    .parse()
                    .context("invalid --chunk-size")?;
            }
            "--help" => {
                println!(
                    "forensics --address <addr> --output <csv> --features <csv> --start-block <n> --end-block <n> --chunk-size <n>\n\nDefaults:\n  --address 0xdab2da14de3aba41adc3b4ede3dbc99ae28a5941\n  --output reports/forensics_dab2da.csv\n  --features (unset)\n  --start-block latest-150000\n  --end-block latest\n  --chunk-size 500"
                );
                std::process::exit(0);
            }
            _ => return Err(anyhow!("unknown argument: {arg}")),
        }
    }

    Ok((
        address,
        std::path::PathBuf::from(output),
        features.map(std::path::PathBuf::from),
        start_block,
        end_block,
        chunk_size,
    ))
}

async fn fetch_latest_block_info(client: &Client, rpc_url: &str) -> Result<(u64, i64)> {
    let payload = serde_json::json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"eth_getBlockByNumber",
        "params":["latest", false]
    });
    let resp: Value = client
        .post(rpc_url)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;
    let result = resp
        .get("result")
        .ok_or_else(|| anyhow!("missing latest block result"))?;
    let block_hex = result
        .get("number")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("missing latest block number"))?;
    let ts_hex = result
        .get("timestamp")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("missing latest block timestamp"))?;
    let block = hex_to_u64(block_hex).context("invalid latest block number")?;
    let ts = hex_to_u64(ts_hex).context("invalid latest block timestamp")? as i64;
    Ok((block, ts))
}

async fn fetch_logs(
    client: &Client,
    rpc_url: &str,
    from_block: u64,
    to_block: u64,
    topic_from: Option<&str>,
    topic_to: Option<&str>,
) -> Result<Vec<RpcLog>> {
    let filter = serde_json::json!({
        "fromBlock": format!("0x{:x}", from_block),
        "toBlock": format!("0x{:x}", to_block),
        "topics": [
            TRANSFER_TOPIC,
            topic_from,
            topic_to,
        ]
    });
    let payload =
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getLogs","params":[filter]});
    let resp: Value = client
        .post(rpc_url)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;
    if let Some(error) = resp.get("error") {
        return Err(anyhow!("rpc error: {}", error));
    }
    let result = resp
        .get("result")
        .ok_or_else(|| anyhow!("missing eth_getLogs result"))?;
    let logs: Vec<RpcLog> =
        serde_json::from_value(result.clone()).context("failed to decode eth_getLogs result")?;
    Ok(logs)
}

async fn process_logs(
    logs: &[RpcLog],
    latest_block: u64,
    latest_ts: i64,
    target_address: &str,
    direction: &str,
    features: Option<&HashMap<i64, OBIStats>>,
) -> Result<Vec<ForensicsRow>> {
    let mut rows = Vec::with_capacity(logs.len());
    for log in logs {
        let block_number = hex_to_u64(&log.block_number).unwrap_or(0);
        let timestamp = estimate_block_timestamp(latest_block, latest_ts, block_number);

        let (from, to) = decode_transfer_topics(&log.topics);
        let obi_estimate = features.and_then(|map| {
            map.get(&timestamp)
                .map(|stats| stats.sum / stats.count as f64)
        });
        rows.push(ForensicsRow {
            tx_hash: log.transaction_hash.clone(),
            block_number,
            timestamp,
            direction: direction.to_string(),
            token_contract: log.address.to_lowercase(),
            value_raw: log.data.clone(),
            log_index: log.log_index.clone(),
            from: from.unwrap_or_else(|| target_address.to_string()),
            to: to.unwrap_or_else(|| target_address.to_string()),
            obi_estimate,
        });
    }
    Ok(rows)
}

fn estimate_block_timestamp(latest_block: u64, latest_ts: i64, block_number: u64) -> i64 {
    if latest_block <= block_number {
        return latest_ts;
    }
    let diff = latest_block - block_number;
    latest_ts.saturating_sub((diff * 3) as i64)
}

fn pad_topic_address(address: &str) -> Result<String> {
    let addr = address.trim_start_matches("0x");
    if addr.len() != 40 {
        return Err(anyhow!("invalid address length"));
    }
    Ok(format!("0x{:0>64}", addr.to_lowercase()))
}

fn decode_transfer_topics(topics: &[String]) -> (Option<String>, Option<String>) {
    let from = topics.get(1).and_then(|value| topic_to_address(value));
    let to = topics.get(2).and_then(|value| topic_to_address(value));
    (from, to)
}

fn topic_to_address(topic: &str) -> Option<String> {
    let raw = topic.trim_start_matches("0x");
    if raw.len() < 40 {
        return None;
    }
    let addr = &raw[raw.len() - 40..];
    Some(format!("0x{}", addr))
}

fn hex_to_u64(value: &str) -> Option<u64> {
    let trimmed = value.trim_start_matches("0x");
    u64::from_str_radix(trimmed, 16).ok()
}

fn is_rate_limited(err: &anyhow::Error) -> bool {
    err.to_string().to_lowercase().contains("limit exceeded")
}

fn shrink_chunk(chunk_size: u64) -> u64 {
    let next = chunk_size / 2;
    if next < 50 {
        50
    } else {
        next
    }
}

fn load_features(path: &Path) -> Result<HashMap<i64, OBIStats>> {
    let mut reader = csv::Reader::from_path(path)
        .with_context(|| format!("open features csv {}", path.display()))?;
    let mut map: HashMap<i64, OBIStats> = HashMap::new();

    for record in reader.records() {
        let record = record?;
        let ts_us: i64 = record
            .get(3)
            .ok_or_else(|| anyhow!("missing ts_us column"))?
            .parse()
            .unwrap_or(0);
        let obi: f64 = record.get(8).unwrap_or("0").parse().unwrap_or(0.0);
        let ts = ts_us / 1_000_000;

        let entry = map.entry(ts).or_default();
        entry.sum += obi;
        entry.count += 1;
    }

    Ok(map)
}

fn write_report(output_path: &Path, rows: &[ForensicsRow]) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create output dir {}", parent.display()))?;
        }
    }

    let mut writer = Writer::from_path(output_path)?;
    writer.write_record([
        "timestamp",
        "block_number",
        "direction",
        "token_contract",
        "value_raw",
        "tx_hash",
        "log_index",
        "from",
        "to",
        "obi_estimate",
    ])?;

    for row in rows {
        writer.write_record([
            row.timestamp.to_string(),
            row.block_number.to_string(),
            row.direction.clone(),
            row.token_contract.clone(),
            row.value_raw.clone(),
            row.tx_hash.clone(),
            row.log_index.clone(),
            row.from.clone(),
            row.to.clone(),
            row.obi_estimate
                .map(|value| format!("{value:.6}"))
                .unwrap_or_else(|| "".to_string()),
        ])?;
    }

    writer.flush()?;
    Ok(())
}
