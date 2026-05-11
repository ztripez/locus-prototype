pub mod accept;
pub mod check;
pub mod debt;
pub mod emit_air;
pub mod explain;
pub mod init;
pub mod paradigms;
pub mod prune;

use anyhow::Result;

use crate::cli::{Cli, Command};

// locus: allow MO005 — this dispatch function is the composition hub for the CLI; its line count
// exceeds the 25-line budget because it must enumerate all 20 paradigm sub-commands. The
// body is pure dispatch (no business logic), making it canonical composition glue.
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::EmitAir(args) => emit_air::run(args),
        Command::Init(args) => init::run(args),
        Command::Check(args) => check::run(args),
        Command::Accept(cmd) => accept::run(cmd),
        Command::Ab(cmd) => paradigms::ab::run(cmd),
        Command::Bo(cmd) => paradigms::bo::run(cmd),
        Command::Cf(cmd) => paradigms::cf::run(cmd),
        Command::Cr(cmd) => paradigms::cr::run(cmd),
        Command::Cx(cmd) => paradigms::cx::run(cmd),
        Command::Da(cmd) => paradigms::da::run(cmd),
        Command::Dc(cmd) => paradigms::dc::run(cmd),
        Command::Dg(cmd) => paradigms::dg::run(cmd),
        Command::Er(cmd) => paradigms::er::run(cmd),
        Command::Fl(cmd) => paradigms::fl::run(cmd),
        Command::Fo(cmd) => paradigms::fo::run(cmd),
        Command::Mo(cmd) => paradigms::mo::run(cmd),
        Command::Ob(cmd) => paradigms::ob::run(cmd),
        Command::Pa(cmd) => paradigms::pa::run(cmd),
        Command::Rm(cmd) => paradigms::rm::run(cmd),
        Command::Rw(cmd) => paradigms::rw::run(cmd),
        Command::Ta(cmd) => paradigms::ta::run(cmd),
        Command::Ut(cmd) => paradigms::ut::run(cmd),
        Command::Debt(args) => debt::run(args),
        Command::Explain(args) => explain::run(args),
        Command::Prune(args) => prune::run(args),
    }
}
