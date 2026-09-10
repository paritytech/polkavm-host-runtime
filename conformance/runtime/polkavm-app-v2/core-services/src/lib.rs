/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]

const RANDOM_OFFSET: usize = 40;
const RANDOM_BYTES: usize = 16;
const OUTPUT_BYTES: usize = RANDOM_OFFSET + RANDOM_BYTES;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_clock_monotonic(destination: u32) -> i32;
    fn polkadot_host_0_1_core_clock_wall(destination: u32) -> i32;
    fn polkadot_host_0_1_core_random(destination: u32, length: u32) -> i32;
    fn host_save_submit(pointer: u32, length: u32) -> u32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    let mut output = [0u8; OUTPUT_BYTES];
    let mut monotonic_first = 0u64;
    let mut monotonic_second = 0u64;
    let mut wall = 0u64;

    let monotonic_first_status = unsafe {
        polkadot_host_0_1_core_clock_monotonic(&mut monotonic_first as *mut u64 as u32)
    };
    let monotonic_second_status = unsafe {
        polkadot_host_0_1_core_clock_monotonic(&mut monotonic_second as *mut u64 as u32)
    };
    let wall_status =
        unsafe { polkadot_host_0_1_core_clock_wall(&mut wall as *mut u64 as u32) };
    let random_status = unsafe {
        polkadot_host_0_1_core_random(
            output.as_mut_ptr().add(RANDOM_OFFSET) as u32,
            RANDOM_BYTES as u32,
        )
    };

    output[0..8].copy_from_slice(&monotonic_first.to_le_bytes());
    output[8..16].copy_from_slice(&monotonic_second.to_le_bytes());
    output[16..24].copy_from_slice(&wall.to_le_bytes());
    output[24..28].copy_from_slice(&monotonic_first_status.to_le_bytes());
    output[28..32].copy_from_slice(&monotonic_second_status.to_le_bytes());
    output[32..36].copy_from_slice(&wall_status.to_le_bytes());
    output[36..40].copy_from_slice(&random_status.to_le_bytes());

    assert_eq!(unsafe { host_save_submit(output.as_ptr() as u32, output.len() as u32) }, 0);
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
