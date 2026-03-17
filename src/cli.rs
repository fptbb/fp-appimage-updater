use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(name = "fp-appimage-updater")]
#[command(about = "Data-Driven AppImage Manager", long_about = None)]
#[command(version)]
pub struct Cli {
    /// Override the default configuration directory
    #[arg(short, long, global = true, value_name = "PATH")]
    pub config: Option<std::path::PathBuf>,

    /// Emit machine-readable JSON instead of human-readable text
    #[arg(long, global = true)]
    pub json: bool,

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
    /// Update fp-appimage-updater itself to the latest GitHub release
    SelfUpdate {
        #[arg(long)]
        /// Also consider pre-release (RC) versions
        pre_release: bool,
    },
    /// Generate shell completions
    Completion {
        /// The shell to generate the completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}
