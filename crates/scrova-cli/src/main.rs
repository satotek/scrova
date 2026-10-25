use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use scrova_cli::{ReplayConfig, replay};
use scrova_storage::{LASTFM_DESTINATION, Store};

#[derive(Debug, Parser)]
#[command(
    name = "scrova-replay",
    version,
    about = "Replay JSON Lines playback events through the Scrova engine"
)]
struct Cli {
    /// JSON Lines file, or "-" to read stdin.
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Write engine events as JSON Lines instead of human-readable text.
    #[arg(long)]
    json: bool,

    /// Enqueue emitted scrobbles into a SQLite queue at this path.
    #[arg(long, value_name = "PATH")]
    store: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("scrova-replay: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let reader: Box<dyn BufRead> = if cli.input == Path::new("-") {
        Box::new(BufReader::new(io::stdin()))
    } else {
        Box::new(BufReader::new(File::open(&cli.input)?))
    };

    let store = cli.store.as_ref().map(Store::open).transpose()?;

    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    let outcome = replay(
        reader,
        &mut writer,
        ReplayConfig { json: cli.json },
        store.as_ref(),
    )?;
    writer.flush()?;

    if let Some(pending) = outcome.pending_count {
        // Keep stdout valid JSON Lines in --json mode; human mode can show the
        // summary alongside the replayed events.
        if cli.json {
            eprintln!("pending_count({LASTFM_DESTINATION}) = {pending}");
        } else {
            writeln!(writer, "pending_count({LASTFM_DESTINATION}) = {pending}")?;
            writer.flush()?;
        }
    }

    Ok(())
}
