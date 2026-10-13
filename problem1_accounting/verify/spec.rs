// SPDX-License-Identifier: GPL-2.0-only
// Included before the annotated executable word operations.
#[verifier::inline]
pub open spec fn n(w: u64, l: u8) -> u64 {
    (w >> if l == 0 { 4u32 } else { 20u32 }) & 65535
}
#[verifier::inline]
pub open spec fn c(w: u64, l: u8) -> bool { w & (1u64 << l) != 0 }
#[verifier::inline]
pub open spec fn d(w: u64, l: u8) -> bool { w & (4u64 << l) != 0 }
#[verifier::inline]
pub open spec fn legal(w: u64) -> bool { w >> 36 == 0 }
#[verifier::inline]
pub open spec fn safe(w: u64) -> bool {
    (n(w, 0) > 0 && !c(w, 0) ==> d(w, 0)) &&
    (n(w, 1) > 0 && !c(w, 1) ==> d(w, 1))
}
#[verifier::inline]
pub open spec fn registered(w: u64, l: u8, r: u64) -> bool {
    n(r, l) == n(w, l) + 1 && n(r, l ^ 1u8) == n(w, l ^ 1u8) &&
    (r & 3) == (w & 3) && d(r, l) == (d(w, l) || !c(w, l)) &&
    d(r, l ^ 1u8) == d(w, l ^ 1u8)
}
#[verifier::inline]
pub open spec fn published(w: u64, coverage: u8, r: u64) -> bool {
    (r & 3) == coverage as u64 && n(r, 0) == n(w, 0) && n(r, 1) == n(w, 1) &&
    d(r, 0) == (d(w, 0) || (n(w, 0) > 0 && coverage & 1 == 0)) &&
    d(r, 1) == (d(w, 1) || (n(w, 1) > 0 && coverage & 2 == 0))
}
#[verifier::inline]
pub open spec fn released(w: u64, l: u8, r: u64) -> bool {
    n(r, l) + 1 == n(w, l) && n(r, l ^ 1u8) == n(w, l ^ 1u8) && (r & 15) == (w & 15)
}
#[verifier::inline]
pub open spec fn retired(w: u64, l: u8, r: u64) -> bool {
    (r & 3) == (w & 3) && n(r, 0) == n(w, 0) && n(r, 1) == n(w, 1) &&
    d(r, l) == (d(w, l) && n(w, l) > 0) && d(r, l ^ 1u8) == d(w, l ^ 1u8)
}

#[verifier::bit_vector]
proof fn register_bound(w: u64, l: u8)
    requires legal(w), l < 2, n(w, l) < 65535,
    ensures w + (1u64 << if l == 0 { 4u32 } else { 20u32 }) <= u64::MAX,
{}

#[verifier::bit_vector]
proof fn release_bound(w: u64, l: u8)
    requires l < 2, n(w, l) > 0,
    ensures w >= (1u64 << if l == 0 { 4u32 } else { 20u32 }),
{}

#[verifier::bit_vector]
proof fn register_lemma(w: u64, l: u8, r: u64)
    requires legal(w), safe(w), l < 2, n(w, l) < 65535,
        r == (if w & (1u64 << l) == 0 {
            ((w + (1u64 << if l == 0 { 4u32 } else { 20u32 })) as u64) | (4u64 << l)
        } else {
            (w + (1u64 << if l == 0 { 4u32 } else { 20u32 })) as u64
        }),
    ensures registered(w, l, r), legal(r), safe(r),
{}

#[verifier::bit_vector]
proof fn publish_lemma(w: u64, coverage: u8, r: u64)
    requires legal(w), safe(w), coverage < 4,
        r == ((w & !3u64) | coverage as u64 |
            (if w & 0xffff0 != 0 && coverage & 1 == 0 { 4u64 } else { 0u64 }) |
            (if w & 0xffff00000 != 0 && coverage & 2 == 0 { 8u64 } else { 0u64 })),
    ensures published(w, coverage, r), legal(r), safe(r),
{}

#[verifier::bit_vector]
proof fn release_lemma(w: u64, l: u8, r: u64)
    requires legal(w), safe(w), l < 2, n(w, l) > 0,
        r == (w - (1u64 << if l == 0 { 4u32 } else { 20u32 })) as u64,
    ensures released(w, l, r), legal(r), safe(r),
{}

#[verifier::bit_vector]
proof fn retire_lemma(w: u64, l: u8, r: u64)
    requires legal(w), safe(w), l < 2,
        r == (if n(w, l) == 0 { w & !(4u64 << l) } else { w }),
    ensures retired(w, l, r), legal(r), safe(r),
{}
