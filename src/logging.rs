use std::path::Path;

use anyhow::{Context, Result, anyhow};
use tracing_subscriber::EnvFilter;

const DEFAULT_APPLICATION_LOG_LEVEL: &str = "info";
const LOG_FILTER_ENVIRONMENT_VARIABLE: &str = "FLOGGING_LOG";
const LOG_FILE_PREFIX: &str = "flogging.log";

pub fn initialize(directory: &Path) -> Result<()> {
    let application_level = std::env::var(LOG_FILTER_ENVIRONMENT_VARIABLE)
        .unwrap_or_else(|_| DEFAULT_APPLICATION_LOG_LEVEL.to_owned());
    let filter = EnvFilter::try_new(format!("warn,flogging={application_level}"))
        .context("FLOGGING_LOG contains an invalid log level")?;
    let file = tracing_appender::rolling::daily(directory, LOG_FILE_PREFIX);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(file)
        .with_ansi(false)
        .with_thread_names(true)
        .try_init()
        .map_err(|error| anyhow!("could not initialize application logging: {error}"))
}
