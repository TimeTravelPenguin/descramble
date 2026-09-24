//! Process-level logging for the CLI and GUI entry points.
//!
//! Call [`init`] once during binary startup. Library consumers can install their
//! own tracing subscriber instead. Logs go to stderr; `RUST_LOG` overrides the
//! default `warn,descramble=info` filter, and `NO_COLOR` disables ANSI formatting.

use std::io::{self, IsTerminal};

use color_eyre::{Result, eyre::Context};
use tracing_subscriber::EnvFilter;

/// Install the application's tracing subscriber once for the current process.
pub fn init() -> Result<()> {
    let filter = match std::env::var("RUST_LOG") {
        Ok(value) => EnvFilter::try_new(value).wrap_err("Invalid RUST_LOG filter")?,
        Err(std::env::VarError::NotPresent) => EnvFilter::new("warn,descramble=info"),
        Err(error) => return Err(error).wrap_err("Could not read RUST_LOG"),
    };

    let use_ansi = io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(use_ansi)
        .try_init()
        .map_err(|error| color_eyre::eyre::eyre!(error.to_string()))
        .wrap_err("Could not initialize logging")
}
