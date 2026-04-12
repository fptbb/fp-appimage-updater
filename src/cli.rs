use anyhow::{Result, anyhow};
use std::path::PathBuf;
use std::str::FromStr;

pub struct CmdInfo {
    pub name: &'static str,
    pub desc: &'static str,
    pub opts: &'static [&'static str],
}

pub const COMMAND_DEFS: &[CmdInfo] = &[
    CmdInfo {
        name: "init",
        desc: "Initialize starter configuration files",
        opts: &["--global", "--app", "--strategy", "--force"],
    },
    CmdInfo {
        name: "validate",
        desc: "Validate application recipe files",
        opts: &[],
    },
    CmdInfo {
        name: "doctor",
        desc: "Run health checks for local setup",
        opts: &[],
    },
    CmdInfo {
        name: "list",
        desc: "List all configured applications and their status",
        opts: &[],
    },
    CmdInfo {
        name: "check",
        desc: "Check for updates",
        opts: &[],
    },
    CmdInfo {
        name: "update",
        desc: "Update applications (all, or specify one)",
        opts: &[
            "--self-update",
            "--debug-download-url <URL>",
            "--debug-version <VER>",
        ],
    },
    CmdInfo {
        name: "remove",
        desc: "Remove an application and its symlink",
        opts: &["-a", "--all"],
    },
    CmdInfo {
        name: "self-update",
        desc: "Update fp-appimage-updater itself",
        opts: &["--pre-release"],
    },
    CmdInfo {
        name: "completion",
        desc: "Generate shell completion scripts",
        opts: &[],
    },
];

pub const GLOBAL_OPTS: &[&str] = &[
    "-c",
    "--config",
    "--json",
    "-h",
    "--help",
    "-V",
    "--version",
];

pub fn print_help() {
    println!(
        "fp-appimage-updater {}
Data-Driven AppImage Manager

USAGE:
    fp-appimage-updater [OPTIONS] <COMMAND>

OPTIONS:
    -c, --config <PATH>  Override the default configuration directory
        --json           Emit machine-readable JSON instead of human-readable text
    -h, --help           Print help information
    -V, --version        Print version information

COMMANDS:",
        env!("CARGO_PKG_VERSION")
    );

    for cmd in COMMAND_DEFS {
        println!("    {:<12} {}", cmd.name, cmd.desc);
        if !cmd.opts.is_empty() {
            println!("        {}", cmd.opts.join(", "));
        }
    }
}

pub struct Cli {
    pub config: Option<PathBuf>,
    pub json: bool,
    pub command: Commands,
}

pub enum Commands {
    Init {
        global: bool,
        app: Option<String>,
        strategy: InitStrategy,
        force: bool,
    },
    Validate {
        app_name: Option<String>,
    },
    Doctor,
    List,
    Check {
        app_name: Option<String>,
    },
    Update {
        app_name: Option<String>,
        self_update: bool,
        debug_download_url: Option<String>,
        debug_version: Option<String>,
    },
    Remove {
        app_name: Option<String>,
        all: bool,
    },
    SelfUpdate {
        pre_release: bool,
    },
    Completion {
        shell: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InitStrategy {
    Direct,
    Forge,
    Script,
}

impl FromStr for InitStrategy {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "direct" => Ok(InitStrategy::Direct),
            "forge" => Ok(InitStrategy::Forge),
            "script" => Ok(InitStrategy::Script),
            _ => Err(anyhow!("Invalid strategy: {}", s)),
        }
    }
}

impl Cli {
    pub fn parse() -> Result<Self> {
        let mut args = pico_args::Arguments::from_env();

        if args.contains(["-h", "--help"]) {
            print_help();
            std::process::exit(0);
        }

        if args.contains(["-V", "--version"]) {
            println!("fp-appimage-updater {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }

        let config: Option<PathBuf> = args
            .opt_value_from_os_str(["-c", "--config"], |s| {
                Ok::<PathBuf, std::convert::Infallible>(PathBuf::from(s))
            })
            .unwrap_or(None);

        let json = args.contains("--json");

        let subcommand = args.subcommand()?.unwrap_or_default();
        let command = match subcommand.as_str() {
            "init" => {
                let global = args.contains("--global");
                let force = args.contains("--force");
                let strategy: Option<InitStrategy> = args.opt_value_from_str("--strategy")?;
                let app: Option<String> = args.opt_value_from_str("--app")?;
                Commands::Init {
                    global,
                    app,
                    strategy: strategy.unwrap_or(InitStrategy::Direct),
                    force,
                }
            }
            "validate" => Commands::Validate {
                app_name: args.opt_free_from_str()?,
            },
            "doctor" => Commands::Doctor,
            "list" => Commands::List,
            "check" => Commands::Check {
                app_name: args.opt_free_from_str()?,
            },
            "update" => {
                let self_update = args.contains("--self-update");
                let debug_download_url: Option<String> =
                    args.opt_value_from_str("--debug-download-url")?;
                let debug_version: Option<String> = args.opt_value_from_str("--debug-version")?;
                Commands::Update {
                    app_name: args.opt_free_from_str()?,
                    self_update,
                    debug_download_url,
                    debug_version,
                }
            }
            "remove" => {
                let all = args.contains(["-a", "--all"]);
                Commands::Remove {
                    app_name: args.opt_free_from_str()?,
                    all,
                }
            }
            "self-update" | "selfupdate" => Commands::SelfUpdate {
                pre_release: args.contains("--pre-release"),
            },
            "completion" => Commands::Completion {
                shell: args
                    .opt_free_from_str()?
                    .unwrap_or_else(|| String::from("bash")),
            },
            _ => {
                print_help();
                std::process::exit(1);
            }
        };

        // Ensure no leftover flags exist
        let remaining = args.finish();
        if !remaining.is_empty() {
            eprintln!("Warning: unused arguments left: {:?}", remaining);
        }

        Ok(Self {
            config,
            json,
            command,
        })
    }
}
