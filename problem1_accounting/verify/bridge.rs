// SPDX-License-Identifier: GPL-2.0-only
// Included with the executable word bodies and the unbounded lifecycle model.
// This connects their state semantics. It does not verify the C CAS binding or
// establish the kernel lifecycle premises for a loaded scheduler.
pub open spec fn linked(w: u64, l: u8, s: Queue) -> bool {
    legal(w) && l < 2 && s.registered == n(w, l) &&
    s.covered == c(w, l) && s.draining == d(w, l)
}

#[verifier::bit_vector]
proof fn same_coverage(w: u64, r: u64, l: u8)
    requires l < 2, (r & 3) == (w & 3),
    ensures c(r, l) == c(w, l),
{}

#[verifier::bit_vector]
proof fn same_flags(w: u64, r: u64, l: u8)
    requires l < 2, (r & 15) == (w & 15),
    ensures c(r, l) == c(w, l), d(r, l) == d(w, l),
{}

#[verifier::bit_vector]
proof fn new_coverage(r: u64, l: u8, coverage: u8)
    requires l < 2, coverage < 4, (r & 3) == coverage as u64,
    ensures c(r, l) == (coverage & (1u8 << l) != 0),
{}

#[verifier::bit_vector]
proof fn published_local(w: u64, r: u64, l: u8, coverage: u8)
    requires l < 2, coverage < 4, published(w, coverage, r),
    ensures n(r, l) == n(w, l),
        d(r, l) == (d(w, l) || (n(w, l) > 0 && coverage & (1u8 << l) == 0)),
{}

pub proof fn connect_register(w: u64, l: u8, r: u64, s: Queue)
    requires linked(w, l, s), inv(s), registered(w, l, r), legal(r),
    ensures linked(r, l, next(s, Step::Register)), inv(next(s, Step::Register)),
{
    same_coverage(w, r, l);
    preservation(s, Step::Register);
}

pub proof fn connect_publish(w: u64, l: u8, coverage: u8, r: u64, s: Queue)
    requires linked(w, l, s), inv(s), coverage < 4, published(w, coverage, r), legal(r),
    ensures linked(r, l, next(s, Step::Publish(coverage & (1u8 << l) != 0))),
        inv(next(s, Step::Publish(coverage & (1u8 << l) != 0))),
{
    new_coverage(r, l, coverage);
    published_local(w, r, l, coverage);
    preservation(s, Step::Publish(coverage & (1u8 << l) != 0));
}

pub proof fn connect_release(w: u64, l: u8, r: u64, s: Queue)
    requires linked(w, l, s), inv(s), enabled(s, Step::Release),
        released(w, l, r), legal(r),
    ensures linked(r, l, next(s, Step::Release)), inv(next(s, Step::Release)),
{
    same_flags(w, r, l);
    preservation(s, Step::Release);
}

pub proof fn connect_retire(w: u64, l: u8, r: u64, s: Queue)
    requires linked(w, l, s), inv(s), retired(w, l, r), legal(r),
    ensures linked(r, l, next(s, Step::Retire)), inv(next(s, Step::Retire)),
{
    same_coverage(w, r, l);
    if l == 0 { } else { assert(l == 1); }
    preservation(s, Step::Retire);
}

pub proof fn connect_kernel_step(w: u64, l: u8, s: Queue, a: Step)
    requires linked(w, l, s), inv(s), enabled(s, a),
        matches!(a, Step::Land | Step::CancelPending | Step::Move | Step::Remove | Step::FailedConsume),
    ensures linked(w, l, next(s, a)), inv(next(s, a)),
{
    preservation(s, a);
    match a {
        Step::Land => {}, Step::CancelPending => {}, Step::Move => {}, Step::Remove => {},
        Step::FailedConsume => {}, _ => {},
    }
}

pub proof fn connected_no_stranding(w: u64, l: u8, s: Queue)
    requires linked(w, l, s), inv(s), s.queued > 0, !c(w, l),
    ensures d(w, l),
{
    no_stranding(s);
}
