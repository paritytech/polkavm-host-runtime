/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]
#![allow(static_mut_refs)]

const EXPECTED_ARGUMENTS: &[u8] =
    b"\x02\0\0\0\x0d\0\0\0shell.polkavm\x07\0\0\0--login";
const EXPECTED_ENVIRONMENT: &[u8] =
    b"\x02\0\0\0\x04\0\0\0HOME\x05\0\0\0/home\x04\0\0\0TERM\x07\0\0\0pvm-tty";
const BUFFER_CAPACITY: usize = 128;
const SUCCESS_STATUS: i32 = 23;

static mut BUFFER: [u8; BUFFER_CAPACITY] = [0; BUFFER_CAPACITY];

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_args(pointer: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_core_environment(pointer: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_core_exit(status: i32);
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    verify_record(read_core_args, EXPECTED_ARGUMENTS);
    verify_record(read_core_environment, EXPECTED_ENVIRONMENT);
    unsafe { polkadot_host_0_1_core_exit(SUCCESS_STATUS) };
}

fn read_core_args(pointer: u32, capacity: u32) -> i32 {
    unsafe { polkadot_host_0_1_core_args(pointer, capacity) }
}

fn read_core_environment(pointer: u32, capacity: u32) -> i32 {
    unsafe { polkadot_host_0_1_core_environment(pointer, capacity) }
}

fn verify_record(read: fn(u32, u32) -> i32, expected: &[u8]) {
    let required = read(0, 0);
    assert_eq!(required, -(expected.len() as i32));

    let written = read(
        unsafe { BUFFER.as_mut_ptr() } as u32,
        unsafe { BUFFER.len() } as u32,
    );
    assert_eq!(written, expected.len() as i32);
    assert_eq!(unsafe { &BUFFER[..written as usize] }, expected);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
