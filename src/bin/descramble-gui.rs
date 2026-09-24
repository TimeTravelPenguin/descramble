fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    descramble::logging::init()?;

    tracing::info!("Starting Descramble");

    descramble::gui::run()?;

    Ok(())
}
