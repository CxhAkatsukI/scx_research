// SPDX-License-Identifier: GPL-2.0
// Execute the C/Rust fragments, scheduling one API/shared operation at a time.
use layered_case::{api::Api, monitor::Log, policy::*};
use std::{cell::RefCell, sync::{mpsc, Arc}, thread};
type Pending = (usize, u8);
type Receipt = (usize, u8, u32);
enum Message { Waiting(Pending), Observed(Receipt), Done(usize) }
struct Hooks { id: usize, tx: mpsc::Sender<Message>, resume: mpsc::Receiver<()> }
thread_local! { static HOOKS: RefCell<Option<Hooks>> = const { RefCell::new(None) }; }
#[no_mangle]
pub extern "C" fn layered_before(kind: u8) {
    HOOKS.with(|h| {
        let h = h.borrow(); let h = h.as_ref().unwrap();
        h.tx.send(Message::Waiting((h.id, kind))).unwrap(); h.resume.recv().unwrap();
    });
}
#[no_mangle]
pub extern "C" fn layered_after(kind: u8, value: u32) {
    HOOKS.with(|h| {
        let h = h.borrow(); let h = h.as_ref().unwrap();
        h.tx.send(Message::Observed((h.id, kind, value))).unwrap();
    });
}
unsafe extern "C" {
    fn source_reset(deferred: bool); fn source_enqueue(); fn source_drain() -> bool;
    fn source_finish(); fn source_count() -> u32; fn source_enabled() -> bool;
}
#[derive(Clone, Copy)]
enum Backend { Rust, C }
#[derive(Debug, PartialEq, Eq)]
struct Result { frontier: Vec<Pending>, receipts: Vec<Receipt>, q: u32, d: bool }
fn run(prefix: &[usize], deferred: bool, backend: Backend) -> Result {
    let api = Arc::new(Api::new(deferred));
    if matches!(backend, Backend::C) { unsafe { source_reset(deferred); } }
    let (tx, rx) = mpsc::channel();
    let mut resumes = Vec::new(); let mut workers = Vec::new();
    for id in 0..2 {
        let (resume, waits) = mpsc::channel(); resumes.push(resume);
        let tx = tx.clone(); let api = api.clone();
        workers.push(thread::spawn(move || {
            HOOKS.with(|h| *h.borrow_mut() = Some(Hooks { id, tx: tx.clone(), resume: waits }));
            match backend {
                Backend::Rust => {
                    let mut log = Log::new(id == 0);
                    if id == 0 { enqueue_tail(&api, &mut log); api.finish_enqueue(); }
                    else { drain_selected(&api, &mut log); }
                },
                Backend::C => unsafe {
                    if id == 0 { source_enqueue(); source_finish(); } else { source_drain(); }
                },
            }
            tx.send(Message::Done(id)).unwrap();
        }));
    }
    drop(tx);
    let mut waiting = [None; 2]; let mut done = [false; 2];
    let mut receipts = Vec::new(); let mut frontier = Vec::new(); let mut step = 0;
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
        assert!(waiting[id].take().is_some(), "illegal prefix {prefix:?}");
        resumes[id].send(()).unwrap(); step += 1;
    }
    for worker in workers { worker.join().unwrap(); }
    let (q, d) = match backend { Backend::Rust => api.snapshot(),
        Backend::C => unsafe { (source_count(), source_enabled()) } };
    Result { frontier, receipts, q, d }
}
fn explore(deferred: bool) {
    let mut prefixes = vec![Vec::new()]; let mut completed = 0; let mut bad = 0;
    while let Some(prefix) = prefixes.pop() {
        let rust = run(&prefix, deferred, Backend::Rust);
        let c = run(&prefix, deferred, Backend::C);
        assert_eq!(rust, c, "C/Rust mismatch, mode={deferred}, prefix={prefix:?}");
        if rust.frontier.is_empty() {
            completed += 1;
            if rust.q > 0 && !rust.d { bad += 1; }
        } else {
            for (id, _) in rust.frontier {
                let mut next = prefix.clone(); next.push(id); prefixes.push(next);
            }
        }
    }
    assert!(completed > 0);
    if deferred { assert!(bad > 0); } else { assert_eq!(bad, 0); }
    println!("{}: {completed} completed schedules, {bad} bad; C/Rust receipts identical",
        if deferred { "deferred (v6.16 enqueue)" } else { "immediate (hypothetical)" });
}
fn main() {
    assert_eq!(std::mem::size_of::<Log>(), 0);
    let witness = [0, 0, 0, 0, 1, 1, 1, 1, 0, 0];
    let r = run(&witness, true, Backend::Rust);
    assert_eq!(r, run(&witness, true, Backend::C));
    assert_eq!((r.q, r.d), (1, false));
    println!("witness: E.request, E.read_C(false), E.enable, E.kick, R.read_Q(1), R.disable, R.consume(true), R.read_Q(0), E.return, K.land");
    explore(false); explore(true);
}
