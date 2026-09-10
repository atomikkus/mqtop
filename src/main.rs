use clap::Parser;
use mqtop_rs::cli::{run, Cli};

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{e:#}");
            // Always restore terminal if somehow left raw — ratatui::restore is idempotent-ish.
            ratatui::restore();
            std::process::exit(2);
        }
    }
}
