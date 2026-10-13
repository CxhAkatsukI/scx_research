// SPDX-License-Identifier: GPL-2.0-only
use problem1_accounting::{word, Error, Partition, TaskRecord};
use std::sync::{Arc, Barrier};

#[test]
fn pending_window_and_failed_consume_cannot_retire_drain() {
    let p = Partition::new(3).unwrap();
    let mut task = TaskRecord::default();
    let ticket = p.enqueue(&mut task, 0).unwrap(); // kernel Q is still zero
    p.config_update(2).unwrap();
    p.drain_after_attempt(0).unwrap(); // real queue can still be empty
    assert_eq!(word::count(p.snapshot(), 0), 1);
    assert_ne!(p.snapshot() & 4, 0);
    assert_eq!(p.finish(&mut task, ticket, false), Err(Error::NotRemoved));
    // The adapter now commits, moves and runs this exact generation.
    p.finish(&mut task, ticket, true).unwrap();
    p.drain_after_attempt(0).unwrap();
    assert_eq!(word::count(p.snapshot(), 0), 0);
    assert_eq!(p.snapshot() & 4, 0);
    assert_eq!(p.finish(&mut task, ticket, true), Err(Error::NotRegistered));
}

#[test]
fn registration_is_bound_to_task_partition_queue_and_generation() {
    let p = Partition::new(0).unwrap();
    let other = Partition::new(0).unwrap();
    let mut a = TaskRecord::default();
    let mut b = TaskRecord::default();
    let first = p.enqueue(&mut a, 0).unwrap();
    let second = p.enqueue(&mut b, 1).unwrap();
    let before = p.snapshot();
    assert_eq!(
        other.finish(&mut a, first, true),
        Err(Error::WrongPartition)
    );
    assert_eq!(p.finish(&mut a, second, true), Err(Error::WrongTask));
    assert_eq!(p.enqueue(&mut a, 1), Err(Error::AlreadyRegistered));
    let mut wrong_queue = first;
    wrong_queue.llc = 1;
    assert_eq!(p.finish(&mut a, wrong_queue, true), Err(Error::InvalidLlc));
    assert_eq!(p.snapshot(), before);
    p.finish(&mut a, first, true).unwrap();
    let next = p.enqueue(&mut a, 1).unwrap();
    assert_eq!(p.finish(&mut a, first, true), Err(Error::WrongGeneration));
    assert_eq!(word::count(p.snapshot(), 1), 2);
    p.finish(&mut a, next, true).unwrap();
    p.finish(&mut b, second, true).unwrap();
}

#[test]
fn moving_partition_does_not_change_ticket_identity() {
    let p = Partition::new(0).unwrap();
    let mut task = TaskRecord::default();
    let ticket = p.enqueue(&mut task, 0).unwrap();
    let p = Box::new(p);
    p.finish(&mut task, ticket, true).unwrap();
    assert_eq!(word::count(p.snapshot(), 0), 0);
}

#[test]
fn saturated_queue_rejects_before_any_insert_request_or_counter_wrap() {
    let p = Partition::new(0).unwrap();
    let mut tasks = Vec::new();
    for _ in 0..word::LIMIT {
        let mut task = TaskRecord::default();
        p.enqueue(&mut task, 1).unwrap();
        tasks.push(task);
    }
    let mut extra = TaskRecord::default();
    let before = p.snapshot();
    assert_eq!(p.enqueue(&mut extra, 1), Err(Error::Full));
    assert!(extra.placement().is_none());
    assert_eq!(p.snapshot(), before);
    assert_eq!(word::count(before, 0), 0);
    assert_eq!(word::count(before, 1), word::LIMIT);
    for mut task in tasks {
        let ticket = task.placement().unwrap();
        p.finish(&mut task, ticket, true).unwrap();
    }
    assert_eq!(word::count(p.snapshot(), 1), 0);
}

#[test]
fn concurrent_callbacks_share_one_cas_order_and_preserve_both_queues() {
    let p = Arc::new(Partition::new(3).unwrap());
    let gate = Arc::new(Barrier::new(5));
    let mut threads = Vec::new();
    for worker in 0..4 {
        let p = Arc::clone(&p);
        let gate = Arc::clone(&gate);
        threads.push(std::thread::spawn(move || {
            gate.wait();
            for round in 0..2000 {
                if worker == 0 {
                    p.config_update((round % 4) as u8).unwrap();
                } else if worker == 1 {
                    p.drain_after_attempt(0).unwrap();
                    p.drain_after_attempt(1).unwrap();
                } else {
                    let mut task = TaskRecord::default();
                    let ticket = p.enqueue(&mut task, (worker % 2) as u8).unwrap();
                    std::thread::yield_now(); // pending, then independently confirmed removal
                    p.finish(&mut task, ticket, true).unwrap();
                }
                let w = p.snapshot();
                for llc in 0..2 {
                    assert!(word::count(w, llc) == 0 || w & (1 << llc) != 0 || w & (4 << llc) != 0);
                }
            }
        }));
    }
    gate.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(word::count(p.snapshot(), 0), 0);
    assert_eq!(word::count(p.snapshot(), 1), 0);
}

#[test]
fn invalid_inputs_do_not_publish_partial_config_or_accounting() {
    let p = Partition::new(1).unwrap();
    let mut task = TaskRecord::default();
    assert_eq!(p.config_update(4), Err(Error::InvalidCoverage));
    assert_eq!(p.enqueue(&mut task, 2), Err(Error::InvalidLlc));
    assert_eq!(p.drain_after_attempt(2), Err(Error::InvalidLlc));
    assert_eq!(p.snapshot(), 1);
}
