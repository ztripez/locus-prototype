mod cli;
mod commands;
mod diff;
mod semantic_facts;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    commands::run(cli::Cli::parse())
}
