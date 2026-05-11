use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};

// locus: ot boundary cli.emit-air cli
#[derive(clap::Args, Debug)]
pub struct EmitAirArgs {
    /// Workspace root (containing Cargo.toml).
    #[arg(long, default_value = ".")]
    pub workspace: PathBuf,
    /// Output file. Defaults to stdout.
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Pretty-print JSON.
    #[arg(long)]
    pub pretty: bool,
}

pub fn run(args: EmitAirArgs) -> Result<()> {
    let air = locus_rust::scan(&args.workspace)
        .with_context(|| format!("scan failed: {}", args.workspace.display()))?;

    let mut writer: Box<dyn Write> = match args.output {
        Some(path) => Box::new(BufWriter::new(
            File::create(&path).with_context(|| format!("create {}", path.display()))?,
        )),
        None => Box::new(BufWriter::new(io::stdout().lock())),
    };

    if args.pretty {
        serde_json::to_writer_pretty(&mut writer, &air)?;
    } else {
        serde_json::to_writer(&mut writer, &air)?;
    }
    writer.write_all(b"\n")?;
    Ok(())
}
