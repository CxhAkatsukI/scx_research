mod adapter;
mod scheduler;
mod types;

use scheduler::{PolicyCore, PolicyInput};
use types::{RunMode, TaskRef};

fn main() {
    let mut policy = PolicyCore::new(RunMode::Deterministic);
    let _actions = policy.step(PolicyInput::Enqueued {
        task: TaskRef::new(100, 1),
        is_problem_workload: true,
    });
}
