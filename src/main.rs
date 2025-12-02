use clap::Parser;

mod cli;
mod models;
mod server;
mod services;
mod tests;
mod utils;

use cli::{Cli, Commands, build_env_filter, list_printers, print_local_file};
use server::run_server;
use services::print_job::fetch_and_print_job_by_id;
use utils::config::load_config;
use utils::tui::run_tui;

#[actix_web::main]
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

                let http_client = reqwest::Client::new();
                match fetch_and_print_job_by_id(job_id, &http_client, &config).await {
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
