use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use problem1_stranded_draining::harness::{run_local, HarnessConfig, RunMode};
use problem1_stranded_draining::trace::trace_to_json;

fn main() -> io::Result<()> {
    let cli = match Cli::parse(env::args().skip(1)) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("{}", Cli::usage());
            std::process::exit(2);
        }
    };

    let run = run_local(&cli.config);
    let json = trace_to_json(&run.events);

    if let Some(path) = cli.write {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, json)?;
    } else {
        io::stdout().write_all(json.as_bytes())?;
    }

    eprintln!(
        "mode={:?} attempts={} invalid_hits={} recovered={} synthetic_topology={}",
        run.summary.mode,
        run.summary.attempts,
        run.summary.invalid_hits,
        run.summary.recovered,
        run.summary.synthetic_topology
    );
    for note in &run.summary.notes {
        eprintln!("note: {note}");
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct Cli {
    config: HarnessConfig,
    write: Option<PathBuf>,
}

impl Cli {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mode = args
            .next()
            .ok_or_else(|| "missing mode".to_string())
            .and_then(|mode| RunMode::parse(&mode))?;
        let mut config = HarnessConfig::new(mode);
        let mut write = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--attempts" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--attempts requires a value".to_string())?;
                    config.attempts = value
                        .parse::<usize>()
                        .map_err(|error| format!("invalid --attempts `{value}`: {error}"))?;
                }
                "--synthetic-topology" => {
                    config.use_synthetic_topology = true;
                }
                "--write" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--write requires a path".to_string())?;
                    write = Some(PathBuf::from(value));
                }
                "--help" | "-h" => {
                    return Err("help requested".to_string());
                }
                _ => return Err(format!("unknown argument `{arg}`")),
            }
        }

        Ok(Self { config, write })
    }

    fn usage() -> &'static str {
        "usage: problem1_harness <report|deterministic|stochastic> [--attempts N] [--synthetic-topology] [--write PATH]"
    }
}
