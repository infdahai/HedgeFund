use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};
use tracing_appender::non_blocking::WorkerGuard;

/// Holds the guards for logging to ensure they aren't dropped prematurely
pub struct TelemetryGuards {
    pub stdout: WorkerGuard,
    pub file: WorkerGuard,
}

/// Initialize the telemetry system with non-blocking logging to both stdout and file.
/// Returns guards that must be held for the duration of the program.
pub fn init_telemetry() -> TelemetryGuards {
    // Create logs directory if it doesn't exist
    let _ = std::fs::create_dir_all("logs");

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

    // 2. File logging (logs/opinion-hft.log.YYYY-MM-DD)
    let file_appender = tracing_appender::rolling::daily("logs", "opinion-hft.log");
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    if let Err(e) = Registry::default()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init()
    {
        eprintln!("Global subscriber already set, skipping init: {}", e);
    }

    TelemetryGuards {
        stdout: stdout_guard,
        file: file_guard,
    }
}
