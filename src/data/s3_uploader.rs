//! S3 Uploader Module for Parquet files
//!
//! This module provides functionality to upload rotated Parquet files to S3.
//! It follows the "Sidecar Pattern" - running as a separate process to avoid
//! blocking the trading hot path.
//!
//! # Design Principles
//! - **Separation of Concerns**: Trading process only writes locally; uploader handles S3
//! - **No Blocking**: Upload runs asynchronously, never blocks trading loop
//! - **Fault Tolerance**: Failed uploads are retried; local files preserved until confirmed
//! - **Credential Isolation**: S3 credentials only in uploader, not trading process

#[cfg(feature = "s3-upload")]
use anyhow::{Context, Result};
#[cfg(feature = "s3-upload")]
use aws_config::BehaviorVersion;
#[cfg(feature = "s3-upload")]
use aws_sdk_s3::Client;
#[cfg(feature = "s3-upload")]
use chrono::{Timelike, Utc};
#[cfg(feature = "s3-upload")]
use std::path::{Path, PathBuf};
#[cfg(feature = "s3-upload")]
use tokio::fs;
#[cfg(feature = "s3-upload")]
use tracing::{debug, error, info, warn};

/// Configuration for S3 uploader
#[cfg(feature = "s3-upload")]
#[derive(Debug, Clone)]
pub struct S3UploaderConfig {
    /// S3 bucket name
    pub bucket: String,
    /// Local parquet directory to scan
    pub parquet_dir: String,
    /// Upload interval in seconds
    pub interval_secs: u64,
    /// Whether to delete local files after successful upload
    pub delete_after_upload: bool,
    /// AWS region (optional, uses default if not set)
    pub region: Option<String>,
}

#[cfg(feature = "s3-upload")]
impl Default for S3UploaderConfig {
    fn default() -> Self {
        Self {
            bucket: "opinion-hft-data".to_string(),
            parquet_dir: "data/parquet".to_string(),
            interval_secs: 300, // 5 minutes
            delete_after_upload: true,
            region: None,
        }
    }
}

#[cfg(feature = "s3-upload")]
impl S3UploaderConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "opinion-hft-data".to_string()),
            parquet_dir: std::env::var("PARQUET_DIR")
                .unwrap_or_else(|_| "data/parquet".to_string()),
            interval_secs: std::env::var("UPLOAD_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),
            delete_after_upload: std::env::var("DELETE_AFTER_UPLOAD")
                .map(|s| s != "false" && s != "0")
                .unwrap_or(true),
            region: std::env::var("AWS_REGION").ok(),
        }
    }
}

/// S3 Uploader handles uploading rotated Parquet files to S3
#[cfg(feature = "s3-upload")]
pub struct S3Uploader {
    client: Client,
    config: S3UploaderConfig,
}

#[cfg(feature = "s3-upload")]
impl S3Uploader {
    /// Create a new S3Uploader with the given configuration
    pub async fn new(config: S3UploaderConfig) -> Result<Self> {
        let aws_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = Client::new(&aws_config);

        Ok(Self { client, config })
    }

    /// Run a single upload cycle
    ///
    /// Scans the parquet directory for completed files (not current hour, not .tmp)
    /// and uploads them to S3.
    pub async fn run_upload_cycle(&self) -> Result<UploadStats> {
        let now = Utc::now();
        let current_hour_str = format!("hour={:02}", now.hour());
        let data_path = Path::new(&self.config.parquet_dir);

        let mut stats = UploadStats::default();

        if !data_path.exists() {
            debug!(path = %self.config.parquet_dir, "Parquet directory does not exist");
            return Ok(stats);
        }

        // Find all parquet files
        let mut files_to_upload = Vec::new();
        self.find_parquet_files(data_path, &mut files_to_upload)
            .await?;

        stats.files_scanned = files_to_upload.len();

        for file_path in files_to_upload {
            let path_str = file_path.to_string_lossy();

            // Skip current hour (file may still be written)
            if path_str.contains(&current_hour_str) {
                debug!(path = %path_str, "Skipping current hour file");
                stats.files_skipped += 1;
                continue;
            }

            // Skip .tmp files (still being written)
            if path_str.ends_with(".tmp") {
                debug!(path = %path_str, "Skipping .tmp file");
                stats.files_skipped += 1;
                continue;
            }

            // Check for _SUCCESS marker in parent directory
            if !self.partition_ready(&file_path) {
                debug!(path = %path_str, "Skipping file without _SUCCESS marker");
                stats.files_skipped += 1;
                continue;
            }

            info!(path = %path_str, "Uploading file");

            match self.upload_file(&file_path).await {
                Ok(_) => {
                    info!(path = %path_str, "Upload successful");
                    stats.files_uploaded += 1;
                    stats.bytes_uploaded +=
                        fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0);

                    if self.config.delete_after_upload {
                        if let Err(e) = fs::remove_file(&file_path).await {
                            error!(path = %path_str, error = %e, "Failed to delete file after upload");
                        } else {
                            debug!(path = %path_str, "Deleted local file after upload");
                        }
                    }
                }
                Err(e) => {
                    error!(path = %path_str, error = %e, "Failed to upload file");
                    stats.files_failed += 1;
                }
            }
        }

        // Clean up empty directories
        self.cleanup_empty_dirs(data_path).await?;

        Ok(stats)
    }

    /// Check if a partition is ready (has _SUCCESS marker)
    fn partition_ready(&self, file_path: &Path) -> bool {
        let Some(parent) = file_path.parent() else {
            return false;
        };
        let success_marker = parent.join("_SUCCESS");
        success_marker.exists()
    }

    /// Recursively find all .parquet files in a directory
    async fn find_parquet_files(&self, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries = fs::read_dir(dir)
            .await
            .with_context(|| format!("Failed to read directory: {:?}", dir))?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                Box::pin(self.find_parquet_files(&path, files)).await?;
            } else if let Some(ext) = path.extension() {
                if ext == "parquet" {
                    files.push(path);
                }
            }
        }
        Ok(())
    }

    /// Upload a single file to S3
    async fn upload_file(&self, path: &Path) -> Result<()> {
        let body = aws_sdk_s3::primitives::ByteStream::from_path(path)
            .await
            .with_context(|| format!("Failed to read file: {:?}", path))?;

        // Calculate S3 key (remove local parquet_dir prefix)
        let key = path
            .strip_prefix(&self.config.parquet_dir)
            .with_context(|| format!("Failed to strip prefix from path: {:?}", path))?
            .to_string_lossy()
            .replace('\\', "/");

        self.client
            .put_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .body(body)
            .send()
            .await
            .with_context(|| format!("Failed to upload to S3: {}", key))?;

        Ok(())
    }

    /// Clean up empty directories after uploads
    async fn cleanup_empty_dirs(&self, dir: &Path) -> Result<()> {
        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };

        let mut is_empty = true;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                Box::pin(self.cleanup_empty_dirs(&path)).await?;
                // Check if directory is now empty after recursive cleanup
                let dir_is_empty = match fs::read_dir(&path).await {
                    Ok(mut d) => d.next_entry().await.ok().flatten().is_none(),
                    Err(_) => true,
                };
                if !dir_is_empty {
                    is_empty = false;
                }
            } else {
                is_empty = false;
            }
        }

        // Don't delete the root parquet directory
        if is_empty && dir != Path::new(&self.config.parquet_dir) {
            if let Err(e) = fs::remove_dir(dir).await {
                warn!(path = ?dir, error = %e, "Failed to remove empty directory");
            }
        }

        Ok(())
    }

    /// Get the current configuration
    pub fn config(&self) -> &S3UploaderConfig {
        &self.config
    }
}

/// Statistics from an upload cycle
#[cfg(feature = "s3-upload")]
#[derive(Debug, Default)]
pub struct UploadStats {
    /// Number of files scanned
    pub files_scanned: usize,
    /// Number of files uploaded successfully
    pub files_uploaded: usize,
    /// Number of files skipped (current hour, .tmp, no _SUCCESS)
    pub files_skipped: usize,
    /// Number of files that failed to upload
    pub files_failed: usize,
    /// Total bytes uploaded
    pub bytes_uploaded: u64,
}

#[cfg(feature = "s3-upload")]
impl std::fmt::Display for UploadStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "scanned={}, uploaded={}, skipped={}, failed={}, bytes={}",
            self.files_scanned,
            self.files_uploaded,
            self.files_skipped,
            self.files_failed,
            self.bytes_uploaded
        )
    }
}

#[cfg(test)]
#[cfg(feature = "s3-upload")]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = S3UploaderConfig::default();
        assert_eq!(config.bucket, "opinion-hft-data");
        assert_eq!(config.parquet_dir, "data/parquet");
        assert_eq!(config.interval_secs, 300);
        assert!(config.delete_after_upload);
    }

    #[test]
    fn test_upload_stats_display() {
        let stats = UploadStats {
            files_scanned: 10,
            files_uploaded: 5,
            files_skipped: 3,
            files_failed: 2,
            bytes_uploaded: 1024,
        };
        let display = format!("{}", stats);
        assert!(display.contains("uploaded=5"));
        assert!(display.contains("failed=2"));
    }
}
