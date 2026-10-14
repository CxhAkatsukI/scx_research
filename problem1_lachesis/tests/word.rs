// SPDX-License-Identifier: GPL-2.0
// Exercise the same erased executable functions that are linked into BPF.
use p1_protocol::word::*;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

fn safe(w: u64) -> bool { count(w) == 0 || w & 1 != 0 || w & 2 != 0 }

fn main() {
    for w in 0..262144 {
        let n = count(w);
        assert_eq!(n, w >> 2);
        for c in [false, true] {
            let r = publish(w, c);
            assert_eq!(count(r), n);
            assert_eq!(r & 1 != 0, c);
            assert!(safe(r));
        }
        if n < LIMIT {
            let r = register(w);
            assert_eq!(count(r), n + 1);
            assert_eq!(r & 1, w & 1);
            assert!(safe(r));
        }
        if n > 0 {
            let r = release(w);
            assert_eq!(count(r), n - 1);
            assert!(!safe(w) || safe(r));
        }
        let r = retire(w);
        assert_eq!(count(r), n);
        assert_eq!(r & 2 != 0, w & 2 != 0 && n > 0);
        assert!(!safe(w) || safe(r));
    }
    // A drain retirement computed from stale empty state cannot overwrite a
    // racing registration. The actual runtime retries using the observed word.
    let shared = AtomicU64::new(2);
    let stale = shared.load(SeqCst);
    assert_eq!(shared.compare_exchange(2, register(2), SeqCst, SeqCst), Ok(2));
    assert_eq!(shared.compare_exchange(stale, retire(stale), SeqCst, SeqCst), Err(6));
    assert!(safe(shared.load(SeqCst)));
    println!("OK: all 262144 packed words and stale retirement CAS");
}
