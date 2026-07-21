use clap::Parser;

mod cli;
mod error;
mod models;
mod server;
mod services;
mod tests;
mod utils;

use cli::{Cli, Commands, build_env_filter, list_printers, print_local_file};
use server::run_server;
use services::print_job::{JobContext, fetch_and_print_job_by_id, new_in_flight_jobs};
use services::printer::new_printer_cache;
use utils::config::load_config;
use utils::http::build_http_client;
use utils::tui::run_tui;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(build_env_filter(cli.verbose))
        .init();

    match cli.command {
        Some(Commands::Config) => {
            run_tui();
            Ok(())
        }
        Some(Commands::Print {
            file,
            printer,
            job_name,
            job,
        }) => {
            if let Some(job_id) = job {
                // Fetch and print job from API
                let config = load_config();
                if config.flux_api_token.is_none() {
                    eprintln!(
                        "Error: No API token configured. Run 'nuxbe-printer-bridge config' first."
                    );
                    std::process::exit(1);
                }

                let ctx = JobContext {
                    http_client: build_http_client(),
                    in_flight_jobs: new_in_flight_jobs(),
                    printer_cache: new_printer_cache(),
                };
                match fetch_and_print_job_by_id(job_id, &config, &ctx).await {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            } else if let Some(ref file_path) = file {
                // Print local file
                print_local_file(file_path, printer.as_deref(), job_name.as_deref());
            }
            Ok(())
        }
        Some(Commands::Printers) => {
            list_printers();
            Ok(())
        }
        _ => run_server(cli.verbose >= 3).await,
    }
}
