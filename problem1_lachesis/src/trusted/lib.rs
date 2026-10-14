// SPDX-License-Identifier: GPL-2.0
#![no_std]

pub mod vprelude {
    #[cfg(verus_keep_ghost)]
    pub use vstd::prelude::*;
    #[cfg(not(verus_keep_ghost))]
    pub use verus_builtin_macros::verus;
}
// Unmodified upstream kernel views, panic path, and kfunc declarations.
#[path = "../../dep/lachesis/src/runtime/trusted/task.rs"]
pub mod task;
#[path = "../../dep/lachesis/src/runtime/trusted/kfunc.rs"]
#[allow(dead_code)]
mod kfunc;
#[path = "../../dep/lachesis/src/runtime/trusted/panic.rs"]
mod panic;
pub mod ops;
pub mod primitives;
pub mod log;
