use std::fs;
use std::path::Path;

use problem1_stranded_draining::linux::{effective_uid, find_in_path};
use problem1_stranded_draining::topology::{discover_from_sysfs, plan_from_topology};

fn main() {
    let mut checks = Vec::new();

    checks.push(check_root());
    checks.push(check_path_exists(
        "sched_ext state file",
        Path::new("/sys/kernel/sched_ext/state"),
    ));
    checks.push(check_readable(
        "sched_ext state readable",
        Path::new("/sys/kernel/sched_ext/state"),
    ));
    checks.push(check_path_exists(
        "kernel BTF",
        Path::new("/sys/kernel/btf/vmlinux"),
    ));
    for tool in ["bpftool", "clang", "cargo", "rustc"] {
        checks.push(check_tool(tool));
    }
    checks.push(check_topology_plan());

    let failed = checks.iter().filter(|check| !check.ok).count();
    for check in &checks {
        println!(
            "{} {:<28} {}",
            if check.ok { "ok " } else { "err" },
            check.name,
            check.note
        );
    }

    if failed > 0 {
        eprintln!("preflight failed: {failed} check(s) need attention before VM loading");
        std::process::exit(1);
    }
}

#[derive(Clone, Debug)]
struct Check {
    name: &'static str,
    ok: bool,
    note: String,
}

fn check_root() -> Check {
    match effective_uid() {
        Ok(0) => Check::ok("effective uid", "running as root".to_string()),
        Ok(uid) => Check::err(
            "effective uid",
            format!("uid={uid}; real sched-ext loading should run via sudo"),
        ),
        Err(error) => Check::err("effective uid", error.to_string()),
    }
}

fn check_path_exists(name: &'static str, path: &Path) -> Check {
    if path.exists() {
        Check::ok(name, path.display().to_string())
    } else {
        Check::err(name, format!("{} is missing", path.display()))
    }
}

fn check_readable(name: &'static str, path: &Path) -> Check {
    match fs::read_to_string(path) {
        Ok(content) => Check::ok(name, format!("current value: {}", content.trim())),
        Err(error) => Check::err(name, error.to_string()),
    }
}

fn check_tool(tool: &'static str) -> Check {
    match find_in_path(tool) {
        Some(path) => Check::ok(tool, path.display().to_string()),
        None => Check::err(tool, "not found in PATH".to_string()),
    }
}

fn check_topology_plan() -> Check {
    match discover_from_sysfs(Path::new("/sys/devices/system/cpu")).and_then(|topology| {
        plan_from_topology(topology)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }) {
        Ok(plan) => Check::ok(
            "topology plan",
            format!(
                "target=CPU{}/LLC{}, recovery=CPU{}/LLC{}, control=CPU{}",
                plan.target_cpu,
                plan.target_llc,
                plan.recovery_cpu,
                plan.recovery_llc,
                plan.control_cpu
            ),
        ),
        Err(error) => Check::err("topology plan", error.to_string()),
    }
}

impl Check {
    fn ok(name: &'static str, note: String) -> Self {
        Self {
            name,
            ok: true,
            note,
        }
    }

    fn err(name: &'static str, note: String) -> Self {
        Self {
            name,
            ok: false,
            note,
        }
    }
}
