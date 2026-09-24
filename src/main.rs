use clap::Parser;
use color_eyre::Result;
use descramble::{cli, logging};

fn main() -> Result<()> {
    color_eyre::install()?;
    logging::init()?;
    cli::run(cli::Cli::parse().command)
}
