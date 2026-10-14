// SPDX-License-Identifier: GPL-2.0
#![no_std]
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;

pub mod word;
pub mod receipts;
#[cfg(verus_keep_ghost)]
pub mod model;
#[cfg(verus_keep_ghost)]
pub mod connection;
#[cfg(verus_keep_ghost)]
pub mod composition;

verus! {
pub const REGISTER: u8 = 0;
pub const RELEASE: u8 = 1;
pub const PUBLISH: u8 = 2;
pub const RETIRE: u8 = 3;
pub const TARGET: u64 = 100;
pub const GLOBAL: u64 = 0x8000_0000_0000_0001;

// These are receipts, not kernel operations invented for this scheduler.
#[derive(PartialEq, Eq)]
pub enum Op {
    TicketRead { owned: bool },
    TicketWrite { owned: bool },
    Cas { kind: u8, before: u64, after: u64, observed: u64 },
    Insert { target: bool },
    Move { moved: bool },
}
}
