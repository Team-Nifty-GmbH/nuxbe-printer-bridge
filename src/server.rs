use std::sync::{Arc, RwLock};

use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::services::print_job::{
    JobContext, job_checker_task, job_status_checker_task, new_in_flight_jobs,
};
use crate::services::printer::{new_printer_cache, printer_checker_task};
use crate::services::websocket::websocket_task;
use crate::utils::config::load_config;
use crate::utils::http::build_http_client;

/// Run the main server application
pub async fn run_server(verbose_debug: bool) -> std::io::Result<()> {
    let config = Arc::new(RwLock::new(load_config()));
    let cancel_token = CancellationToken::new();
    let ctx = JobContext {
        http_client: build_http_client(),
        in_flight_jobs: new_in_flight_jobs(),
        printer_cache: new_printer_cache(),
    };

    // The printer checker's first iteration runs immediately and doubles as
    // startup initialization of the printer cache and stored printer state.
    let handles = vec![
        tokio::spawn(printer_checker_task(
            ctx.printer_cache.clone(),
            config.clone(),
            ctx.http_client.clone(),
            cancel_token.clone(),
            verbose_debug,
        )),
        tokio::spawn(job_checker_task(
            config.clone(),
            ctx.clone(),
            cancel_token.clone(),
        )),
        tokio::spawn(websocket_task(
            config.clone(),
            ctx.clone(),
            cancel_token.clone(),
        )),
        tokio::spawn(job_status_checker_task(
            config.clone(),
            ctx.clone(),
            cancel_token.clone(),
        )),
    ];

    info!("Print server started");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received, stopping background tasks...");

    // Signal all tasks to stop
    cancel_token.cancel();

    // Wait for all tasks to finish (with timeout)
    for handle in handles {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
    }

    info!("Shutdown complete");
    Ok(())
}
