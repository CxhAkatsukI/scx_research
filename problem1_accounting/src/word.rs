// SPDX-License-Identifier: GPL-2.0-only
// One partition, two LLCs. C: bits 0..1, D: bits 2..3,
// N[0]: bits 4..19, N[1]: bits 20..35. Upper bits must be zero.
// `//%` lines are proof annotations activated by scripts/verify.py.

pub const LIMIT: u64 = 65535;

pub fn count(w: u64, llc: u8) -> u64
//% requires llc < 2,
//% ensures r == n(w, llc),
{
    (w >> if llc == 0 { 4 } else { 20 }) & 65535
}

pub fn register(w: u64, llc: u8) -> u64
//% requires legal(w), safe(w), llc < 2, n(w, llc) < 65535,
//% ensures registered(w, llc, r), legal(r), safe(r),
{
    let shift: u32 = if llc == 0 { 4 } else { 20 };
    //% proof { register_bound(w, llc); }
    let r = w + (1u64 << shift);
    let r = if w & (1u64 << llc) == 0 {
        r | (4u64 << llc)
    } else {
        r
    };
    //% proof { register_lemma(w, llc, r); }
    r
}

pub fn publish(w: u64, coverage: u8) -> u64
//% requires legal(w), safe(w), coverage < 4,
//% ensures published(w, coverage, r), legal(r), safe(r),
{
    let r = (w & !3u64)
        | coverage as u64
        | (if w & 0xffff0 != 0 && coverage & 1 == 0 {
            4u64
        } else {
            0u64
        })
        | (if w & 0xffff00000 != 0 && coverage & 2 == 0 {
            8u64
        } else {
            0u64
        });
    //% proof { publish_lemma(w, coverage, r); }
    r
}

pub fn release(w: u64, llc: u8) -> u64
//% requires legal(w), safe(w), llc < 2, n(w, llc) > 0,
//% ensures released(w, llc, r), legal(r), safe(r),
{
    let shift: u32 = if llc == 0 { 4 } else { 20 };
    //% proof { release_bound(w, llc); }
    let r = w - (1u64 << shift);
    //% proof { release_lemma(w, llc, r); }
    r
}

pub fn retire(w: u64, llc: u8) -> u64
//% requires legal(w), safe(w), llc < 2,
//% ensures retired(w, llc, r), legal(r), safe(r),
{
    let shift: u32 = if llc == 0 { 4 } else { 20 };
    let r = if (w >> shift) & 65535 == 0 {
        w & !(4u64 << llc)
    } else {
        w
    };
    //% proof { retire_lemma(w, llc, r); }
    r
}
