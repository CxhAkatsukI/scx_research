use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::protocol::{Cpu, CpuId, LlcId, CPU0, CPU1, CPU2, LLC0, LLC1};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopologyPlan {
    pub topology: Vec<Cpu>,
    pub target_cpu: CpuId,
    pub target_llc: LlcId,
    pub recovery_cpu: CpuId,
    pub recovery_llc: LlcId,
    pub control_cpu: CpuId,
    pub synthetic: bool,
    pub notes: Vec<String>,
}

impl TopologyPlan {
    pub fn example() -> Self {
        Self {
            topology: vec![
                Cpu {
                    id: CPU0,
                    llc: LLC0,
                },
                Cpu {
                    id: CPU1,
                    llc: LLC1,
                },
                Cpu {
                    id: CPU2,
                    llc: LLC1,
                },
            ],
            target_cpu: CPU0,
            target_llc: LLC0,
            recovery_cpu: CPU1,
            recovery_llc: LLC1,
            control_cpu: CPU2,
            synthetic: true,
            notes: vec!["using synthetic CPU0/LLC0, CPU1/LLC1, CPU2/control topology".to_string()],
        }
    }

    pub fn workload_cpus(&self) -> [CpuId; 2] {
        [self.target_cpu, self.recovery_cpu]
    }

    pub fn initial_partition_cpus(&self) -> [CpuId; 2] {
        [self.target_cpu, self.recovery_cpu]
    }

    pub fn updated_partition_cpus(&self) -> [CpuId; 1] {
        [self.recovery_cpu]
    }
}

pub fn discover_or_example() -> TopologyPlan {
    match discover_from_sysfs(Path::new("/sys/devices/system/cpu")) {
        Ok(topology) => match plan_from_topology(topology) {
            Ok(plan) => plan,
            Err(error) => {
                let mut plan = TopologyPlan::example();
                plan.notes.push(format!(
                    "sysfs topology was not usable for a safe dry-run plan: {error}"
                ));
                plan
            }
        },
        Err(error) => {
            let mut plan = TopologyPlan::example();
            plan.notes.push(format!(
                "sysfs topology was not usable for a safe dry-run plan: {error}"
            ));
            plan
        }
    }
}

pub fn discover_from_sysfs(cpu_root: &Path) -> io::Result<Vec<Cpu>> {
    let online = read_cpu_set(cpu_root.join("online"))
        .or_else(|_| read_cpu_set(cpu_root.join("present")))
        .or_else(|_| discover_cpu_dirs(cpu_root))?;

    let mut llc_groups = BTreeMap::<Vec<CpuId>, LlcId>::new();
    let mut topology = Vec::new();

    for cpu_id in online {
        let group = read_llc_group(cpu_root, cpu_id).unwrap_or_else(|| vec![cpu_id]);
        let next_llc = llc_groups.len() as LlcId;
        let llc = *llc_groups.entry(group).or_insert(next_llc);
        topology.push(Cpu { id: cpu_id, llc });
    }

    topology.sort_by_key(|cpu| cpu.id);
    Ok(topology)
}

pub fn plan_from_topology(mut topology: Vec<Cpu>) -> Result<TopologyPlan, String> {
    topology.sort_by_key(|cpu| cpu.id);
    topology.dedup_by_key(|cpu| cpu.id);

    if topology.len() < 3 {
        return Err("need at least three CPUs: target, recovery, and control".to_string());
    }

    let target = topology
        .first()
        .copied()
        .ok_or_else(|| "topology is empty".to_string())?;

    let recovery = topology
        .iter()
        .copied()
        .find(|cpu| cpu.llc != target.llc)
        .ok_or_else(|| "need at least two LLC groups for the stranding experiment".to_string())?;

    let control = topology
        .iter()
        .copied()
        .find(|cpu| cpu.id != target.id && cpu.id != recovery.id)
        .ok_or_else(|| "need a CPU outside the workload pair for harness/control".to_string())?;

    Ok(TopologyPlan {
        topology,
        target_cpu: target.id,
        target_llc: target.llc,
        recovery_cpu: recovery.id,
        recovery_llc: recovery.llc,
        control_cpu: control.id,
        synthetic: false,
        notes: vec![format!(
            "selected CPU{} / LLC{} as orphan target, CPU{} / LLC{} as recovery, CPU{} as control",
            target.id, target.llc, recovery.id, recovery.llc, control.id
        )],
    })
}

pub fn parse_cpu_list(input: &str) -> Result<Vec<CpuId>, String> {
    let mut cpus = BTreeSet::new();

    for part in input.trim().split(',').filter(|part| !part.is_empty()) {
        if let Some((start, end)) = part.split_once('-') {
            let start = parse_cpu_id(start)?;
            let end = parse_cpu_id(end)?;
            if start > end {
                return Err(format!("invalid descending CPU range: {part}"));
            }
            for cpu in start..=end {
                cpus.insert(cpu);
            }
        } else {
            cpus.insert(parse_cpu_id(part)?);
        }
    }

    if cpus.is_empty() {
        return Err("CPU list is empty".to_string());
    }

    Ok(cpus.into_iter().collect())
}

fn parse_cpu_id(input: &str) -> Result<CpuId, String> {
    input
        .parse::<CpuId>()
        .map_err(|error| format!("invalid CPU id `{input}`: {error}"))
}

fn read_cpu_set(path: PathBuf) -> io::Result<Vec<CpuId>> {
    let content = fs::read_to_string(path)?;
    parse_cpu_list(&content).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn discover_cpu_dirs(cpu_root: &Path) -> io::Result<Vec<CpuId>> {
    let mut cpus = Vec::new();

    for entry in fs::read_dir(cpu_root)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(cpu_id) = file_name.strip_prefix("cpu") else {
            continue;
        };
        if cpu_id.chars().all(|c| c.is_ascii_digit()) {
            cpus.push(
                parse_cpu_id(cpu_id)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            );
        }
    }

    cpus.sort_unstable();
    if cpus.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no cpuN directories found",
        ))
    } else {
        Ok(cpus)
    }
}

fn read_llc_group(cpu_root: &Path, cpu_id: CpuId) -> Option<Vec<CpuId>> {
    let path = cpu_root
        .join(format!("cpu{cpu_id}"))
        .join("cache")
        .join("index3")
        .join("shared_cpu_list");
    let content = fs::read_to_string(path).ok()?;
    parse_cpu_list(&content).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges_and_singletons() {
        assert_eq!(parse_cpu_list("0-2,4,6-7").unwrap(), vec![0, 1, 2, 4, 6, 7]);
    }

    #[test]
    fn rejects_descending_range() {
        assert!(parse_cpu_list("3-1").is_err());
    }

    #[test]
    fn picks_target_recovery_and_control() {
        let plan = plan_from_topology(vec![
            Cpu { id: 0, llc: 0 },
            Cpu { id: 1, llc: 1 },
            Cpu { id: 2, llc: 1 },
        ])
        .unwrap();

        assert_eq!(plan.target_cpu, 0);
        assert_eq!(plan.target_llc, 0);
        assert_eq!(plan.recovery_cpu, 1);
        assert_eq!(plan.control_cpu, 2);
        assert!(!plan.synthetic);
    }
}
