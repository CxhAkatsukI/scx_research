// SPDX-License-Identifier: GPL-2.0-only
//! The BPF object links these actual verified bodies, without a C rewrite.
#![no_std]
#[path = "../src/word.rs"]
#[allow(dead_code)]
mod word;

#[no_mangle]
pub extern "C" fn p1_count(w: u64, llc: u8) -> u64 { word::count(w, llc) }
#[no_mangle]
pub extern "C" fn p1_register(w: u64, llc: u8) -> u64 { word::register(w, llc) }
#[no_mangle]
pub extern "C" fn p1_publish(w: u64, coverage: u8) -> u64 { word::publish(w, coverage) }
#[no_mangle]
pub extern "C" fn p1_release(w: u64, llc: u8) -> u64 { word::release(w, llc) }
#[no_mangle]
pub extern "C" fn p1_retire(w: u64, llc: u8) -> u64 { word::retire(w, llc) }
