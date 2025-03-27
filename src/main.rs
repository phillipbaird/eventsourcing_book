use anyhow::Context;
use cart_server::{configure_tracing, construct_app_state, domain::cart::cart_items_from_db_read_model_reset, infra::{get_config_settings, Cli}, start_server};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let settings = get_config_settings().context("Could not read application configuration.")?;

    // _worker_guard is pulled back into the scope of main() to ensure all tracing events get
    // written to the log file when the program terminates, which is done when _worker_guard is
    // dropped.
    let _worker_guard = configure_tracing(&settings);

    let app_state = construct_app_state(settings).await?;
   
    if cli.reset_cart_items {
        cart_items_from_db_read_model_reset(&app_state.pool).await?;
    }

    start_server(app_state).await
}
