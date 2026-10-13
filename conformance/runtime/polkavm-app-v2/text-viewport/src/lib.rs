/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Negotiates the Host-retained text viewport the way a real editor guest
//! does: the first submit carries a text-viewport command; a Host without a
//! renderer rejects it, and the guest falls back to a plain output.

#![no_std]

/// Cursor-icon-only output with one text-viewport command: rows 10..12 with a
/// bold `let` run and an empty row 11; cursor at (11, 3).
const VIEWPORT: [u8; 150] = [
    80, 85, 73, 49, 1, 0, 48, 0, 150, 0, 0, 0, 1, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 94, 0, 0, 0, 1, 0, 0, 0,
    10, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 11, 0, 0, 0, 3, 0, 0, 0, 0, 0, 64, 66, 0, 0, 0, 66, 0,
    0, 32, 68, 0, 0, 240, 67, 0, 0, 160, 65, 0, 0, 96, 65, 0, 0, 0, 0, 10, 0, 0, 0, 10, 0, 2, 0,
    108, 101, 116, 32, 120, 32, 61, 32, 49, 59, 3, 0, 1, 0, 255, 0, 0, 255, 7, 0, 0, 0, 255, 0,
    255, 0, 11, 0, 0, 0, 0, 0, 0, 0,
];
/// The same cursor icon without the viewport command.
const PLAIN: [u8; 48] = [
    80, 85, 73, 49, 1, 0, 48, 0, 48, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_ui_output_submit(pointer: u32, length: u32) -> u32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    let status = unsafe { host_ui_output_submit(VIEWPORT.as_ptr() as u32, VIEWPORT.len() as u32) };
    if status == 1 {
        let fallback = unsafe { host_ui_output_submit(PLAIN.as_ptr() as u32, PLAIN.len() as u32) };
        assert_eq!(fallback, 0);
    } else {
        assert_eq!(status, 0);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
