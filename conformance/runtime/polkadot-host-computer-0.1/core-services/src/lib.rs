/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]
#![allow(static_mut_refs)]

const SUCCESS_STATUS: i32 = 31;
const STATUS_INVALID: i32 = -3;
const STATUS_LIMIT: i32 = -6;
const YEAR_2020_NS: u64 = 1_577_836_800_000_000_000;

static mut FIRST_RANDOM: [u8; 32] = [0; 32];
static mut SECOND_RANDOM: [u8; 32] = [0; 32];

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_clock_monotonic(destination: u32) -> i32;
    fn polkadot_host_0_1_core_clock_wall(destination: u32) -> i32;
    fn polkadot_host_0_1_core_random(pointer: u32, length: u32) -> i32;
    fn polkadot_host_0_1_core_exit(status: i32);
}

fn finish(status: i32) -> ! {
    unsafe { polkadot_host_0_1_core_exit(status) };
    loop {
        core::hint::spin_loop();
    }
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    let mut monotonic_first = 0u64;
    let mut monotonic_second = 0u64;
    if unsafe {
        polkadot_host_0_1_core_clock_monotonic(&mut monotonic_first as *mut u64 as u32)
    } != 0
        || unsafe {
            polkadot_host_0_1_core_clock_monotonic(&mut monotonic_second as *mut u64 as u32)
        } != 0
        || monotonic_second < monotonic_first
    {
        finish(1);
    }

    let mut wall = 0u64;
    if unsafe { polkadot_host_0_1_core_clock_wall(&mut wall as *mut u64 as u32) } != 0
        || wall < YEAR_2020_NS
    {
        finish(2);
    }

    let first_status = unsafe {
        polkadot_host_0_1_core_random(
            FIRST_RANDOM.as_mut_ptr() as u32,
            FIRST_RANDOM.len() as u32,
        )
    };
    let second_status = unsafe {
        polkadot_host_0_1_core_random(
            SECOND_RANDOM.as_mut_ptr() as u32,
            SECOND_RANDOM.len() as u32,
        )
    };
    if first_status != 0 {
        finish(3);
    }
    if second_status != 0 {
        finish(4);
    }
    if unsafe { FIRST_RANDOM == SECOND_RANDOM } {
        finish(5);
    }
    if unsafe { polkadot_host_0_1_core_random(0, 0) } != STATUS_INVALID {
        finish(6);
    }
    if unsafe { polkadot_host_0_1_core_random(FIRST_RANDOM.as_mut_ptr() as u32, 4097) }
        != STATUS_LIMIT
    {
        finish(7);
    }
    finish(SUCCESS_STATUS);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
