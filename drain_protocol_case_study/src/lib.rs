// SPDX-License-Identifier: GPL-2.0
#![no_std]
#[cfg(all(not(verus_keep_ghost), not(host_replay)))]
compile_error!("This case study has only a host replay backend, not a kernel backend.");
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;

pub mod monitor;
pub mod primitives;
pub mod policy;
#[cfg(verus_keep_ghost)]
pub mod model;
#[cfg(verus_keep_ghost)]
pub mod composition;
