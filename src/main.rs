use clap::Parser;

mod cli;
mod models;
mod server;
mod services;
mod tests;
mod utils;

use cli::{Cli, Commands, build_env_filter, list_printers, print_local_file};
use server::run_server;
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
        }) => {
            print_local_file(&file, printer.as_deref(), job_name.as_deref());
            Ok(())
        }
        Some(Commands::Printers) => {
            list_printers();
            Ok(())
        }
        _ => run_server(cli.verbose >= 3).await,
    }
}
