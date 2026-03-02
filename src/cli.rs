use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(name = "fp-appimage-updater")]
#[command(about = "Data-Driven AppImage Manager", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all configured applications and their status
    List,
    /// Check for updates
    Check {
        /// Optional specific application to check
        app_name: Option<String>,
    },
    /// Update applications (all, or specify one)
    Update {
        /// Optional specific application to update
        app_name: Option<String>,
    },
    /// Remove an application and its symlink
    Remove {
        /// Application to remove, or none to remove all
        app_name: Option<String>,
        
        #[arg(short, long)]
        /// Remove all applications
        all: bool,
    },
    /// Generate shell completions
    Completion {
        /// The shell to generate the completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}
