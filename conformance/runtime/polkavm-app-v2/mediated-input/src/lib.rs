/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Registers and triggers a bounded camera-UR input, then saves the decoded
//! bytes delivered by the Host. The first save contains the positive handle,
//! trigger result, and active status as little-endian `i32` values. The second
//! contains the successful read length followed by the decoded bytes.
//! It idles between Host events. Pointer deltas control the capture lifecycle:
//! x=0 cancels, x=1 cancels and re-arms, and x=2 triggers the idle registration.

#![no_std]
#![allow(static_mut_refs)]

const KIND: &[u8] = b"camera-ur";
const MEDIA_TYPE: &[u8] = b"x-test-payload";
const MAX_BYTES: usize = 32;

static mut HANDLE: u32 = 0;
static mut RESULT: [u8; MAX_BYTES + 4] = [0; MAX_BYTES + 4];

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_input_register(
        kind_pointer: u32,
        kind_length: u32,
        media_type_pointer: u32,
        media_type_length: u32,
        max_bytes: u32,
    ) -> i32;
    fn host_input_trigger(handle: u32) -> u32;
    fn host_input_status(handle: u32) -> u32;
    fn host_input_read(handle: u32, pointer: u32, capacity: u32) -> i32;
    fn host_input_cancel(handle: u32) -> u32;
    fn host_poll_input(pointer: u32, capacity: u32) -> u32;
    fn host_update_after(delay_ms: u32);
    fn host_save_submit(pointer: u32, length: u32) -> u32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    unsafe {
        host_update_after(u32::MAX);
        let handle = host_input_register(
            KIND.as_ptr() as u32,
            KIND.len() as u32,
            MEDIA_TYPE.as_ptr() as u32,
            MEDIA_TYPE.len() as u32,
            MAX_BYTES as u32,
        );
        HANDLE = handle.max(0) as u32;
        let trigger = host_input_trigger(HANDLE);
        let status = host_input_status(HANDLE);
        let mut report = [0u8; 12];
        report[..4].copy_from_slice(&handle.to_le_bytes());
        report[4..8].copy_from_slice(&trigger.to_le_bytes());
        report[8..].copy_from_slice(&status.to_le_bytes());
        host_save_submit(report.as_ptr() as u32, report.len() as u32);
    }
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    unsafe {
        host_update_after(u32::MAX);
        let mut input = [0u8; 8];
        if host_poll_input(input.as_mut_ptr() as u32, input.len() as u32) == 8
            && input[0] == 6
        {
            if input[2] != 2 {
                host_input_cancel(HANDLE);
            }
            if input[2] != 0 {
                host_input_trigger(HANDLE);
            }
            return;
        }
        if host_input_status(HANDLE) != 3 {
            return;
        }
        let required = host_input_read(HANDLE, RESULT[4..].as_mut_ptr() as u32, 0);
        if required >= 0 || required.unsigned_abs() as usize > MAX_BYTES {
            return;
        }
        let length = host_input_read(
            HANDLE,
            RESULT[4..].as_mut_ptr() as u32,
            required.unsigned_abs(),
        );
        if length <= 0 {
            return;
        }
        RESULT[..4].copy_from_slice(&length.to_le_bytes());
        host_save_submit(RESULT.as_ptr() as u32, length as u32 + 4);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
