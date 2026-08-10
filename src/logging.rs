use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

pub fn init() -> Result<PathBuf> {
    let directories = ProjectDirs::from("", "", "opencode-tui-rust")
        .context("unable to determine an application data directory")?;
    let log_directory = directories.data_local_dir().join("logs");
    fs::create_dir_all(&log_directory).context("failed to create log directory")?;
    let path = log_directory.join("opencode-tui-rust.jsonl");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("failed to open structured log file")?;
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("opencode_tui_rust=info"));

    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_env_filter(filter)
        .with_writer(file)
        .try_init()
        .map_err(|error| anyhow!("failed to initialize structured logging: {error}"))?;

    Ok(path)
}
