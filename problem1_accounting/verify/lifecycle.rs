// SPDX-License-Identifier: GPL-2.0-only
use vstd::prelude::*;
verus! {

// Counts describe one queue. Detached includes local-DSQ work and removed
// generations waiting for a serialized safe callback to release their ticket.
pub struct Queue {
    pub pending: nat,
    pub queued: nat,
    pub detached: nat,
    pub registered: nat,
    pub covered: bool,
    pub draining: bool,
}

pub open spec fn inv(s: Queue) -> bool {
    s.pending + s.queued + s.detached == s.registered &&
    (s.registered > 0 && !s.covered ==> s.draining)
}

pub enum Step { Register, Land, CancelPending, Move, Remove, Release, Publish(bool), Retire, FailedConsume }

pub open spec fn enabled(s: Queue, a: Step) -> bool {
    match a {
        Step::Land | Step::CancelPending => s.pending > 0,
        Step::Move | Step::Remove => s.queued > 0,
        Step::Release => s.detached > 0,
        _ => true,
    }
}

pub open spec fn next(s: Queue, a: Step) -> Queue {
    match a {
        Step::Register => Queue { pending: s.pending + 1, registered: s.registered + 1,
            draining: s.draining || !s.covered, ..s },
        Step::Land => Queue { pending: (s.pending - 1) as nat, queued: s.queued + 1, ..s },
        // Requires kernel confirmation that this generation cannot land later.
        Step::CancelPending => Queue { pending: (s.pending - 1) as nat,
            detached: s.detached + 1, ..s },
        Step::Move | Step::Remove => Queue { queued: (s.queued - 1) as nat,
            detached: s.detached + 1, ..s },
        Step::Release => Queue { detached: (s.detached - 1) as nat,
            registered: (s.registered - 1) as nat, ..s },
        Step::Publish(c) => Queue { covered: c,
            draining: s.draining || (s.registered > 0 && !c), ..s },
        Step::Retire => Queue { draining: s.draining && s.registered > 0, ..s },
        Step::FailedConsume => s,
    }
}

pub proof fn init(covered: bool)
    ensures inv(Queue { pending: 0, queued: 0, detached: 0,
        registered: 0, covered, draining: false }),
{}

pub proof fn preservation(s: Queue, a: Step)
    requires inv(s), enabled(s, a),
    ensures inv(next(s, a)),
{
    match a {
        Step::Register => {}, Step::Land => {}, Step::CancelPending => {}, Step::Move => {},
        Step::Remove => {}, Step::Release => {}, Step::Publish(_) => {},
        Step::Retire => {}, Step::FailedConsume => {},
    }
}

// Stronger than the stable-state target, conditional on the accounting relation.
pub proof fn no_stranding(s: Queue)
    requires inv(s), s.queued > 0, !s.covered,
    ensures s.draining,
{}
}
