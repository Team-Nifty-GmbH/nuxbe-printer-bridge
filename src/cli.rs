use clap::{ArgAction, Parser, Subcommand};
use printers::common::base::job::PrinterJobOptions;
use printers::{get_printer_by_name, get_printers};
use std::path::Path;
use tracing_subscriber::EnvFilter;

/// Command line arguments for the application
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose debug logging
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the server normally
    Run,

    /// Configure application settings using a text-based UI
    Config,

    /// Print a file to a specified printer, or fetch and print a job from the API
    Print {
        /// Path to the file to print (required unless --job is specified)
        #[arg(short, long, required_unless_present = "job")]
        file: Option<String>,

        /// Name of the printer to use (uses default printer if not specified)
        #[arg(short, long)]
        printer: Option<String>,

        /// Job name (optional)
        #[arg(short = 'n', long)]
        job_name: Option<String>,

        /// Fetch and print a specific job by ID from the API
        #[arg(short = 'j', long)]
        job: Option<u32>,
    },

    /// List available printers
    Printers,
}

/// Build the tracing env filter based on verbosity level
pub fn build_env_filter(verbose: u8) -> EnvFilter {
    match verbose {
        0 => EnvFilter::from_default_env()
            .add_directive("reverb_rs=warn".parse().unwrap())
            .add_directive("nuxbe_printer_bridge=info".parse().unwrap()),
        1 => EnvFilter::from_default_env()
            .add_directive("reverb_rs=info".parse().unwrap())
            .add_directive("nuxbe_printer_bridge=info".parse().unwrap()),
        2 => EnvFilter::from_default_env()
            .add_directive("reverb_rs=debug".parse().unwrap())
            .add_directive("nuxbe_printer_bridge=debug".parse().unwrap()),
        _ => EnvFilter::from_default_env()
            .add_directive("reverb_rs=trace".parse().unwrap())
            .add_directive("nuxbe_printer_bridge=trace".parse().unwrap()),
    }
}

/// Print a local file to a printer
pub fn print_local_file(
    file_path: &str,
    printer_name: Option<&str>,
    job_name: Option<&str>,
) -> bool {
    if !Path::new(file_path).exists() {
        eprintln!("Error: File '{}' not found", file_path);
        std::process::exit(1);
    }

    let printer = if let Some(name) = printer_name {
        match get_printer_by_name(name) {
            Some(p) => p,
            None => {
                eprintln!("Error: Printer '{}' not found", name);
                eprintln!("Available printers:");
                for p in get_printers() {
                    eprintln!("  - {}", p.name);
                }
                std::process::exit(1);
            }
        }
    } else {
        let mut printers = get_printers();
        match printers.pop() {
            Some(p) => p,
            None => {
                eprintln!("Error: No printers available");
                std::process::exit(1);
            }
        }
    };

    let job_name_str = job_name.unwrap_or("CLI Print Job");
    let job_options = PrinterJobOptions {
        name: Some(job_name_str),
        ..PrinterJobOptions::none()
    };

    match printer.print_file(file_path, job_options) {
        Ok(job_id) => {
            println!("Print job submitted successfully");
            println!("  Printer: {}", printer.name);
            println!("  File: {}", file_path);
            println!("  CUPS Job ID: {}", job_id);
            true
        }
        Err(e) => {
            eprintln!("Error: Failed to print file: {:?}", e);
            std::process::exit(1);
        }
    }
}

/// List available printers
pub fn list_printers() {
    let printers = get_printers();
    if printers.is_empty() {
        println!("No printers available");
        return;
    }

    println!("Available printers:");
    for printer in printers {
        println!("  {} - {}", printer.name, printer.system_name);
    }
}
