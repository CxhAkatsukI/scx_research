// SPDX-License-Identifier: GPL-2.0
// Arbitrarily many accounting/empty-retirement calls, no consumption or config.
use crate::{*, monitor::*};
verus! {
pub struct State {
    pub n: nat,
    pub drain: bool,
    pub pcs: Seq<Pc>,
    pub enqueue: Seq<bool>,
    pub credits: Set<int>,
    pub returned: Set<int>,
    pub landed: Set<int>,
}
pub open spec fn slot(s: State, k: int) -> bool { 0 <= k < s.pcs.len() }
pub open spec fn owes(p: Pc) -> bool {
    p == Pc::Counted || p == Pc::Enable || p == Pc::Recheck || p == Pc::Repair
}
pub open spec fn valid_pc(enq: bool, p: Pc) -> bool {
    if enq { p == Pc::Enq || p == Pc::Counted || p == Pc::Enable || p == Pc::Kick || p == Pc::Done }
    else { p == Pc::Scan || p == Pc::Clear || p == Pc::Recheck || p == Pc::Repair
        || p == Pc::Rekick || p == Pc::Done }
}
pub open spec fn shape(s: State) -> bool {
    &&& s.enqueue.len() == s.pcs.len()
    &&& s.n == s.credits.len()
    &&& forall|k: int| #[trigger] s.credits.contains(k)
        <==> slot(s, k) && s.enqueue[k] && s.pcs[k] != Pc::Enq
    &&& forall|k: int| #[trigger] s.returned.contains(k) ==> slot(s, k) && s.pcs[k] == Pc::Done
    &&& s.landed.subset_of(s.credits) && s.landed.subset_of(s.returned)
    &&& forall|k: int| #![trigger s.pcs[k]] slot(s, k)
        ==> valid_pc(s.enqueue[k], s.pcs[k])
}
pub open spec fn inv(s: State) -> bool {
    shape(s) && (s.n > 0 && !s.drain ==> exists|k: int| #![auto] slot(s, k) && owes(s.pcs[k]))
}
pub open spec fn initial(kinds: Seq<bool>) -> State {
    State { n: 0, drain: true, enqueue: kinds,
        pcs: Seq::new(kinds.len(), |k: int| if kinds[k] { Pc::Enq } else { Pc::Scan }),
        credits: Set::empty(), returned: Set::empty(), landed: Set::empty() }
}
pub open spec fn coherent(s: State, o: Op) -> bool {
    match o {
        Op::Inc => s.n < u32::MAX,
        Op::Covered(c) => !c, // This case starts after removal and freezes coverage false.
        Op::Pending(n) => n == s.n,
        _ => true,
    }
}
pub open spec fn enabled(s: State, k: int, o: Op, fixed: bool) -> bool {
    slot(s, k) && !s.returned.contains(k) && allowed(s.pcs[k], o, fixed) && coherent(s, o)
}
pub open spec fn next(s: State, k: int, o: Op, fixed: bool) -> State {
    State {
        n: if o == Op::Inc { s.n + 1 } else { s.n },
        drain: if o == Op::Set { true } else if o == Op::Clear { false } else { s.drain },
        pcs: s.pcs.update(k, advance(s.pcs[k], o, fixed)),
        credits: if o == Op::Inc { s.credits.insert(k) } else { s.credits },
        ..s
    }
}
pub open spec fn stable(s: State) -> bool {
    forall|k: int| slot(s, k) ==> #[trigger] s.pcs[k] == Pc::Done
}
pub open spec fn bad(s: State) -> bool { s.landed.len() > 0 && !s.drain }

pub proof fn initialize(kinds: Seq<bool>)
    ensures inv(initial(kinds)),
{
    let s = initial(kinds);
    assert forall|k: int| #[trigger] s.credits.contains(k)
        <==> slot(s, k) && s.enqueue[k] && s.pcs[k] != Pc::Enq by {}
}

pub proof fn preserves(s: State, k: int, o: Op)
    requires inv(s), enabled(s, k, o, true),
    ensures inv(next(s, k, o, true)),
{
    let r = next(s, k, o, true);
    if o == Op::Inc {
        assert(!s.credits.contains(k));
    }
    assert forall|j: int| #[trigger] r.credits.contains(j)
        <==> slot(r, j) && r.enqueue[j] && r.pcs[j] != Pc::Enq by {
        assert(s.credits.contains(j) <==> slot(s, j) && s.enqueue[j] && s.pcs[j] != Pc::Enq);
    }
    assert forall|j: int| #[trigger] r.returned.contains(j)
        implies slot(r, j) && r.pcs[j] == Pc::Done by {
        assert(s.returned.contains(j) ==> slot(s, j) && s.pcs[j] == Pc::Done);
    }
    assert forall|j: int| #![trigger r.pcs[j]] slot(r, j)
        implies valid_pc(r.enqueue[j], r.pcs[j]) by {
        assert(valid_pc(s.enqueue[j], s.pcs[j]));
    }
    assert(s.landed.subset_of(r.credits));
    assert(shape(r));
    if r.n > 0 && !r.drain {
        if o == Op::Inc || o == Op::Clear {
            assert(owes(r.pcs[k]));
            assert(slot(r, k));
            assert(exists|j: int| #![auto] slot(r, j) && owes(r.pcs[j]));
        } else if s.n > 0 && !s.drain {
            let j = choose|j: int| #![auto] slot(s, j) && owes(s.pcs[j]);
            if j == k {
                assert(owes(r.pcs[k]));
            } else {
                assert(r.pcs[j] == s.pcs[j]);
            }
            assert(slot(r, j));
            assert(owes(r.pcs[j]));
            assert(exists|z: int| #![auto] slot(r, z) && owes(r.pcs[z]));
        }
    }
}

pub proof fn stable_is_safe(s: State)
    requires inv(s), stable(s),
    ensures !bad(s), s.n > 0 ==> s.drain,
{
    if s.n > 0 && !s.drain {
        let k = choose|k: int| #![auto] slot(s, k) && owes(s.pcs[k]);
        assert(s.pcs[k] == Pc::Done);
    }
    vstd::set_lib::lemma_len_subset(s.landed, s.credits);
}
}
