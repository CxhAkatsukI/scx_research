// SPDX-License-Identifier: GPL-2.0
#![no_std]
#[cfg(all(not(verus_keep_ghost), not(host_replay)))]
compile_error!("This source-fragment study has no BPF backend; use Verus or host_replay.");
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::verus;
pub mod monitor;
pub mod api;
pub mod policy;
#[cfg(verus_keep_ghost)]
pub mod model;
#[cfg(verus_keep_ghost)]
pub mod composition;
