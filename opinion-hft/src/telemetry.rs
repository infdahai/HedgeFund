use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};
use tracing_appender::non_blocking::WorkerGuard;

/// Initialize the telemetry system with non-blocking logging.
/// Returns a WorkerGuard that must be held for the duration of the program
/// to ensure logs are flushed.
pub fn init_telemetry() -> WorkerGuard {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,opinion_hft=debug"));

    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());

    // Use a compact format for performance
    let formatting_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_thread_ids(true)
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        .compact();

    Registry::default()
        .with(env_filter)
        .with(formatting_layer)
        .init();

    guard
}
