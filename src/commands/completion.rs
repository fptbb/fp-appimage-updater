use crate::cli::Cli;
use anyhow::Result;
use clap::CommandFactory;
use clap_complete::Shell;

pub fn run(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(())
}
