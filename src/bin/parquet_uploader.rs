//! Parquet Uploader Sidecar Binary
//!
//! This is a standalone daemon that uploads rotated Parquet files to S3.
//! It runs independently from the trading process to avoid blocking the hot path.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                              EC2 Trading Node                                │
//! │                                                                              │
//! │  ┌─────────────────────┐                    ┌─────────────────────────────┐  │
//! │  │   Trading Process   │                    │     Parquet Uploader        │  │
//! │  │   (main.rs)         │                    │     (this binary)           │  │
//! │  │                     │                    │                             │  │
//! │  │   Writes to:        │     watches        │   1. Scan for .parquet      │  │
//! │  │   data/parquet/     │ ────────────────▶  │   2. Skip current hour      │  │
//! │  │   ├─ date=.../      │                    │   3. Upload to S3           │  │
//! │  │   └─ hour=.../      │                    │   4. Delete local on success│  │
//! │  └─────────────────────┘                    └──────────────┬──────────────┘  │
//! │                                                            │                 │
//! └────────────────────────────────────────────────────────────┼─────────────────┘
//!                                                              │
//!                                                              ▼
//!                                               ┌──────────────────────────────┐
//!                                               │         S3 Bucket            │
//!                                               │  orderbook_snapshots_v1/     │
//!                                               │  scan_stats_v1/              │
//!                                               └──────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```bash
//! # Build with s3-upload feature
//! cargo build --release --bin parquet_uploader --features s3-upload
//!
//! # Run with environment variables
//! S3_BUCKET=opinion-hft-data \
//! PARQUET_DIR=data/parquet \
//! UPLOAD_INTERVAL_SECS=300 \
//!   ./target/release/parquet_uploader
//!
//! # Or use AWS IAM Role (recommended on EC2)
//! # No AWS_ACCESS_KEY_ID needed - uses instance metadata
//! ```
//!
//! # Environment Variables
//!
//! | Variable | Default | Description |
//! |----------|---------|-------------|
//! | `S3_BUCKET` | `opinion-hft-data` | S3 bucket name |
//! | `PARQUET_DIR` | `data/parquet` | Local parquet directory |
//! | `UPLOAD_INTERVAL_SECS` | `300` | Seconds between upload cycles |
//! | `DELETE_AFTER_UPLOAD` | `true` | Delete local files after upload |
//! | `AWS_REGION` | (auto) | AWS region (uses SDK default) |
//! | `RUST_LOG` | `info` | Log level |
//! | `UPLOAD_MODE` | `daemon` | `daemon` (loop) or `once` (single run) |

use anyhow::Result;
#[cfg(feature = "s3-upload")]
use std::time::Duration;
#[cfg(feature = "s3-upload")]
use tokio::time;
#[cfg(not(feature = "s3-upload"))]
use tracing::error;
#[cfg(feature = "s3-upload")]
use tracing::{error, info};

#[cfg(feature = "s3-upload")]
use opinion_hft::data::s3_uploader::{S3Uploader, S3UploaderConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    #[cfg(not(feature = "s3-upload"))]
    {
        error!("This binary requires the 's3-upload' feature. Rebuild with:");
        error!("  cargo build --bin parquet_uploader --features s3-upload");
        std::process::exit(1);
    }

    #[cfg(feature = "s3-upload")]
    {
        run_uploader().await
    }
}

#[cfg(feature = "s3-upload")]
async fn run_uploader() -> Result<()> {
    let config = S3UploaderConfig::from_env();

    info!(
        bucket = %config.bucket,
        parquet_dir = %config.parquet_dir,
        interval_secs = config.interval_secs,
        delete_after_upload = config.delete_after_upload,
        "Starting Parquet Uploader Sidecar"
    );

    let uploader = S3Uploader::new(config.clone()).await?;

    // Check upload mode
    let mode = std::env::var("UPLOAD_MODE").unwrap_or_else(|_| "daemon".to_string());

    if mode == "once" {
        // Single run mode (for testing or cron)
        info!("Running in single-shot mode");
        match uploader.run_upload_cycle().await {
            Ok(stats) => {
                info!(%stats, "Upload cycle completed");
                if stats.files_failed > 0 {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                error!(error = %e, "Upload cycle failed");
                std::process::exit(1);
            }
        }
    } else {
        // Daemon mode (continuous loop)
        info!("Running in daemon mode");
        let mut interval = time::interval(Duration::from_secs(config.interval_secs));

        // Run immediately on startup, then on interval
        loop {
            interval.tick().await;

            match uploader.run_upload_cycle().await {
                Ok(stats) => {
                    if stats.files_uploaded > 0 || stats.files_failed > 0 {
                        info!(%stats, "Upload cycle completed");
                    } else {
                        info!("No files to upload");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Upload cycle failed");
                    // Continue running in daemon mode
                }
            }
        }
    }

    Ok(())
}
