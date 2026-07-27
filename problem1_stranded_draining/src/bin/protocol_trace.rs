use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use problem1_stranded_draining::trace::{deterministic_protocol_trace, trace_to_json};

fn main() -> io::Result<()> {
    let args = env::args().collect::<Vec<_>>();
    let json = trace_to_json(&deterministic_protocol_trace());

    match args.as_slice() {
        [_program] => {
            io::stdout().write_all(json.as_bytes())?;
        }
        [_program, flag, path] if flag == "--write" => {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, json)?;
        }
        [program, ..] => {
            eprintln!("usage: {program} [--write PATH]");
            std::process::exit(2);
        }
        [] => unreachable!("argv always contains the program name"),
    }

    Ok(())
}
