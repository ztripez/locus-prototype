mod cli;
mod commands;
mod diff;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    commands::run(cli::Cli::parse())
}
