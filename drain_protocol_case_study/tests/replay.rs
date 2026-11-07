// SPDX-License-Identifier: GPL-2.0
// Stateless schedule exploration: run the actual functions, pausing at primitives.
use drain_case::{monitor::Log, policy::*, primitives::*};
use std::{cell::RefCell, sync::{mpsc, Arc}, thread};

type Pending = (usize, u8);
type Receipt = (usize, u8, u32);
enum Message { Waiting(Pending), Observed(Receipt), Done(usize) }
struct Hooks { id: usize, tx: mpsc::Sender<Message>, resume: mpsc::Receiver<()> }
thread_local! { static HOOKS: RefCell<Option<Hooks>> = const { RefCell::new(None) }; }

#[no_mangle]
pub extern "C" fn case_before(kind: u8) {
    HOOKS.with(|h| {
        let h = h.borrow();
        let h = h.as_ref().unwrap();
        h.tx.send(Message::Waiting((h.id, kind))).unwrap();
        h.resume.recv().unwrap();
    });
}
#[no_mangle]
pub extern "C" fn case_after(kind: u8, value: u32) {
    HOOKS.with(|h| {
        let h = h.borrow();
        let h = h.as_ref().unwrap();
        h.tx.send(Message::Observed((h.id, kind, value))).unwrap();
    });
}
unsafe extern "C" {
    fn before_reset(); fn before_enqueue(); fn before_retire();
    fn before_count() -> u32; fn before_drain() -> bool;
    fn after_reset(); fn after_enqueue(); fn after_retire();
    fn after_count() -> u32; fn after_drain() -> bool;
}

#[derive(Clone, Copy)]
enum Backend { Rust, SourceC }
#[derive(Debug, PartialEq, Eq)]
struct Result {
    frontier: Vec<Pending>, receipts: Vec<Receipt>, count: u32, drain: bool,
}

fn run(prefix: &[usize], fixed: bool, backend: Backend) -> Result {
    let s = Arc::new(Shared { nr_queued: Counter::new(0), llcs_with_cpus: Coverage::new(false),
        llcs_to_drain: DrainMask::new(true) });
    if matches!(backend, Backend::SourceC) {
        unsafe { if fixed { after_reset(); } else { before_reset(); } }
    }
    let (tx, rx) = mpsc::channel();
    let mut resumes = Vec::new();
    let mut workers = Vec::new();
    for id in 0..2 {
        let (resume, waits) = mpsc::channel();
        resumes.push(resume);
        let tx = tx.clone();
        let s = s.clone();
        workers.push(thread::spawn(move || {
            HOOKS.with(|h| *h.borrow_mut() = Some(Hooks { id, tx: tx.clone(), resume: waits }));
            match backend {
                Backend::Rust => {
                    let mut log = Log::new(id == 0, fixed);
                    if id == 0 { account_enqueue(&s, &mut log); }
                    else { empty_queue_branch(&s, fixed, &mut log); }
                },
                Backend::SourceC => unsafe {
                    match (fixed, id) {
                        (false, 0) => before_enqueue(), (false, _) => before_retire(),
                        (true, 0) => after_enqueue(), (true, _) => after_retire(),
                    }
                },
            }
            tx.send(Message::Done(id)).unwrap();
        }));
    }
    drop(tx);
    let mut waiting = [None; 2];
    let mut done = [false; 2];
    let mut receipts = Vec::new();
    let mut frontier = Vec::new();
    let mut step = 0;
    loop {
        while (0..2).any(|id| !done[id] && waiting[id].is_none()) {
            match rx.recv().unwrap() {
                Message::Waiting((id, kind)) => waiting[id] = Some(kind),
                Message::Observed(r) => receipts.push(r),
                Message::Done(id) => done[id] = true,
            }
        }
        if step == prefix.len() {
            frontier = (0..2).filter_map(|id| waiting[id].map(|kind| (id, kind))).collect();
        }
        if done.iter().all(|v| *v) { break; }
        let id = if step < prefix.len() { prefix[step] }
            else { (0..2).find(|id| waiting[*id].is_some()).unwrap() };
        assert!(waiting[id].take().is_some(), "illegal schedule prefix {prefix:?}");
        resumes[id].send(()).unwrap();
        step += 1;
    }
    for worker in workers { worker.join().unwrap(); }
    let (count, drain) = match backend {
        Backend::Rust => (s.nr_queued.snapshot(), s.llcs_to_drain.snapshot()),
        Backend::SourceC => unsafe {
            if fixed { (after_count(), after_drain()) } else { (before_count(), before_drain()) }
        },
    };
    Result { frontier, receipts, count, drain }
}

fn explore(fixed: bool) {
    let mut prefixes = vec![Vec::new()];
    let mut completed = 0;
    let mut bad = 0;
    while let Some(prefix) = prefixes.pop() {
        let rust = run(&prefix, fixed, Backend::Rust);
        let c = run(&prefix, fixed, Backend::SourceC);
        assert_eq!(rust, c, "C/Rust mismatch at prefix {prefix:?}, fixed={fixed}");
        if rust.frontier.is_empty() {
            completed += 1;
            if rust.count > 0 && !rust.drain { bad += 1; }
        } else {
            for (id, _) in rust.frontier {
                let mut next = prefix.clone();
                next.push(id);
                prefixes.push(next);
            }
        }
    }
    assert!(completed > 0);
    if fixed { assert_eq!(bad, 0); } else { assert!(bad > 0); }
    println!("{}: {completed} completed primitive schedules; {bad} bad; C/Rust receipts identical",
        if fixed { "fixed" } else { "historical" });
}

fn main() {
    assert_eq!(std::mem::size_of::<Log>(), 0, "ghost monitor must erase");
    // E=0, R=1: R observes empty, E completes, R clears the still-enabled bit.
    let witness = [1, 0, 0, 0, 0, 1];
    let old = run(&witness, false, Backend::Rust);
    assert_eq!((old.count, old.drain), (1, false));
    assert_eq!(old, run(&witness, false, Backend::SourceC));
    let fixed = run(&witness, true, Backend::Rust);
    assert_eq!((fixed.count, fixed.drain), (1, true));
    assert_eq!(fixed, run(&witness, true, Backend::SourceC));
    println!("historical witness: R.read(0), E.inc, E.read_C(false), E.set, E.kick, R.clear");
    println!("fixed continuation: R.read(1), R.set, R.kick");
    explore(false);
    explore(true);
}
