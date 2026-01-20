use std::env;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

/// Holds the guards for logging to ensure they aren't dropped prematurely
pub struct TelemetryGuards {
    pub stdout: WorkerGuard,
    pub file: Option<WorkerGuard>,
}

/// Initialize the telemetry system with non-blocking logging to both stdout and file.
/// Returns guards that must be held for the duration of the program.
pub fn init_telemetry() -> TelemetryGuards {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,opinion_hft=debug"));

    // 1. Stdout logging (real-time monitoring)
    let (stdout_writer, stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(stdout_writer)
        .with_thread_ids(true)
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        .compact();

    let log_file_enabled = env::var("OPINION_LOG_FILE_ENABLED")
        .ok()
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(true);
    let log_dir = env::var("OPINION_LOG_DIR").unwrap_or_else(|_| "logs".to_string());

    let (file_layer, file_guard) = if log_file_enabled {
        let _ = std::fs::create_dir_all(&log_dir);
        let file_appender = tracing_appender::rolling::daily(&log_dir, "opinion-hft.log");
        let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(file_writer)
            .with_ansi(false)
            .with_thread_ids(true)
            .with_target(true)
            .with_file(true)
            .with_line_number(true);
        (Some(file_layer), Some(file_guard))
    } else {
        (None, None)
    };

    let registry = Registry::default().with(env_filter).with(stdout_layer);
    let init_result = if let Some(file_layer) = file_layer {
        registry.with(file_layer).try_init()
    } else {
        registry.try_init()
    };
    if let Err(e) = init_result {
        eprintln!("Global subscriber already set, skipping init: {}", e);
    }

    TelemetryGuards {
        stdout: stdout_guard,
        file: file_guard,
    }
}
