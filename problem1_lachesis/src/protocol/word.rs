// SPDX-License-Identifier: GPL-2.0
// One atomic word: C = bit 0, D = bit 1, N = bits 2..17.
use crate::*;

verus! {
pub const LIMIT: u64 = 65535;
pub open spec fn n(w: u64) -> int { ((w >> 2) & 65535) as int }
pub open spec fn c(w: u64) -> bool { w & 1 != 0 }
pub open spec fn d(w: u64) -> bool { w & 2 != 0 }
pub open spec fn legal(w: u64) -> bool { w < 262144 }
pub open spec fn safe(w: u64) -> bool { n(w) > 0 && !c(w) ==> d(w) }

pub fn count(w: u64) -> (r: u64)
    ensures r as int == n(w), r <= LIMIT,
{
    let r = (w >> 2) & 65535;
    assert(r <= 65535) by(bit_vector) requires r == ((w >> 2) & 65535);
    r
}

pub fn register(w: u64) -> (r: u64)
    requires legal(w), n(w) < LIMIT,
    ensures legal(r), n(r) == n(w) + 1, c(r) == c(w),
        d(r) == (d(w) || !c(w)), safe(w) ==> safe(r),
        r == ((w + 4) as u64 | if c(w) { 0u64 } else { 2u64 }),
{
    assert(w <= 262139) by(bit_vector)
        requires w < 262144, ((w >> 2) & 65535) < 65535;
    let r = (w + 4) | if w & 1 == 0 { 2 } else { 0 };
    assert(legal(r) && n(r) == n(w) + 1 && c(r) == c(w)
        && d(r) == (d(w) || !c(w))) by(bit_vector)
        requires w < 262144, ((w >> 2) & 65535) < 65535,
            r == ((w + 4u64) as u64) | (if w & 1u64 == 0 { 2u64 } else { 0u64 });
    r
}

pub fn release(w: u64) -> (r: u64)
    requires legal(w), n(w) > 0,
    ensures legal(r), n(r) + 1 == n(w), c(r) == c(w), d(r) == d(w),
        safe(w) ==> safe(r),
        r == (w - 4) as u64,
{
    assert(w >= 4) by(bit_vector) requires ((w >> 2) & 65535) > 0;
    let r = w - 4;
    assert(legal(r) && n(r) + 1 == n(w) && c(r) == c(w) && d(r) == d(w))
        by(bit_vector) requires w < 262144, ((w >> 2) & 65535) > 0, r == w - 4u64;
    r
}

pub fn publish(w: u64, covered: bool) -> (r: u64)
    requires legal(w),
    ensures legal(r), n(r) == n(w), c(r) == covered,
        d(r) == (d(w) || (n(w) > 0 && !covered)), safe(r),
        r == ((w & !1u64) | if covered { 1u64 } else if w >> 2 != 0 { 2u64 } else { 0u64 }),
{
    let r = (w & !1u64) | if covered { 1 } else if w >> 2 != 0 { 2 } else { 0 };
    assert(legal(r) && n(r) == n(w) && c(r) == covered
        && d(r) == (d(w) || (n(w) > 0 && !covered))) by(bit_vector)
        requires w < 262144,
            r == (w & !1u64) | (if covered { 1u64 } else if w >> 2 != 0 { 2u64 } else { 0u64 });
    r
}

pub fn retire(w: u64) -> (r: u64)
    requires legal(w),
    ensures legal(r), n(r) == n(w), c(r) == c(w),
        d(r) == (d(w) && n(w) > 0), safe(w) ==> safe(r),
        r == if w >> 2 == 0 { w & !2u64 } else { w },
{
    let r = if w >> 2 == 0 { w & !2u64 } else { w };
    assert(legal(r) && n(r) == n(w) && c(r) == c(w)
        && d(r) == (d(w) && n(w) > 0)) by(bit_vector)
        requires w < 262144, r == (if w >> 2 == 0 { w & !2u64 } else { w });
    r
}
}
