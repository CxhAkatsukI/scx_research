use std::env;
use std::io;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use problem1_stranded_draining::linux::{
    atomic_write, current_affinity, current_scheduler_policy, current_thread_id,
    set_current_affinity, set_current_sched_ext, SCHED_EXT,
};
use problem1_stranded_draining::topology::parse_cpu_list;

fn main() -> io::Result<()> {
    let cli = match Cli::parse(env::args().skip(1)) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("{}", Cli::usage());
            std::process::exit(2);
        }
    };

    if let Some(cpus) = &cli.cpu_list {
        set_current_affinity(cpus)?;
    }

    if cli.sched_ext {
        set_current_sched_ext()?;
        let policy = current_scheduler_policy()?;
        let affinity = current_affinity()?;
        eprintln!(
            "problem1_workload_ready pid={} tid={} policy={} sched_ext_policy={} affinity={}",
            std::process::id(),
            current_thread_id(),
            policy,
            SCHED_EXT,
            format_cpu_list(&affinity),
        );
        if cli.initial_sleep_ms > 0 {
            thread::sleep(Duration::from_millis(cli.initial_sleep_ms));
        }
    }

    let mut counter = 0_u64;
    loop {
        counter = counter.wrapping_add(1);

        if counter % cli.write_every == 0 {
            atomic_write(&cli.progress_file, &format!("{counter}\n"))?;
            if cli.stop_file.exists() {
                break;
            }
        }

        if cli.max_iters.is_some_and(|max_iters| counter >= max_iters) {
            break;
        }
    }

    atomic_write(&cli.progress_file, &format!("{counter}\n"))?;
    Ok(())
}

#[derive(Clone, Debug)]
struct Cli {
    progress_file: PathBuf,
    stop_file: PathBuf,
    cpu_list: Option<Vec<u16>>,
    sched_ext: bool,
    initial_sleep_ms: u64,
    write_every: u64,
    max_iters: Option<u64>,
}

impl Cli {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut progress_file = None;
        let mut stop_file = None;
        let mut cpu_list = None;
        let mut sched_ext = false;
        let mut initial_sleep_ms = 0;
        let mut write_every = 1_000_000;
        let mut max_iters = None;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--progress-file" => {
                    progress_file = Some(PathBuf::from(next_value(&mut args, "--progress-file")?));
                }
                "--stop-file" => {
                    stop_file = Some(PathBuf::from(next_value(&mut args, "--stop-file")?));
                }
                "--cpu-list" => {
                    let value = next_value(&mut args, "--cpu-list")?;
                    cpu_list = Some(parse_cpu_list(&value)?);
                }
                "--sched-ext" => {
                    sched_ext = true;
                }
                "--initial-sleep-ms" => {
                    let value = next_value(&mut args, "--initial-sleep-ms")?;
                    initial_sleep_ms = value.parse::<u64>().map_err(|error| {
                        format!("invalid --initial-sleep-ms `{value}`: {error}")
                    })?;
                }
                "--write-every" => {
                    let value = next_value(&mut args, "--write-every")?;
                    write_every = parse_nonzero_u64("--write-every", &value)?;
                }
                "--max-iters" => {
                    let value = next_value(&mut args, "--max-iters")?;
                    max_iters = Some(parse_nonzero_u64("--max-iters", &value)?);
                }
                "--help" | "-h" => {
                    return Err("help requested".to_string());
                }
                _ => return Err(format!("unknown argument `{arg}`")),
            }
        }

        Ok(Self {
            progress_file: progress_file
                .ok_or_else(|| "--progress-file is required".to_string())?,
            stop_file: stop_file.ok_or_else(|| "--stop-file is required".to_string())?,
            cpu_list,
            sched_ext,
            initial_sleep_ms,
            write_every,
            max_iters,
        })
    }

    fn usage() -> &'static str {
        "usage: problem1_workload --progress-file PATH --stop-file PATH [--cpu-list LIST] [--sched-ext] [--initial-sleep-ms N] [--write-every N] [--max-iters N]"
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_nonzero_u64(flag: &str, value: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|error| format!("invalid {flag} `{value}`: {error}"))?;
    if parsed == 0 {
        Err(format!("{flag} must be greater than zero"))
    } else {
        Ok(parsed)
    }
}

fn format_cpu_list(cpus: &[u16]) -> String {
    cpus.iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
