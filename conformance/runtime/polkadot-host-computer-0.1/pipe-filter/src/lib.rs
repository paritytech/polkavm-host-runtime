/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Conformance filter for the `polkadot-host-computer/0.1` pipe capabilities.
//! Reads its input stream to EOF, uppercases ASCII, writes the result, and
//! exits 0. Run piped, it is the canonical `upper` filter package.

#![no_std]

const WOULD_BLOCK: i32 = -1;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_yield();
    fn polkadot_host_0_1_core_exit(status: i32);
    fn polkadot_host_0_1_tty_current() -> u32;
    fn polkadot_host_0_1_tty_read(handle: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_tty_write(handle: u32, source: u32, length: u32) -> i32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    unsafe {
        let tty = polkadot_host_0_1_tty_current();
        loop {
            let mut buffer = [0u8; 4096];
            let read =
                polkadot_host_0_1_tty_read(tty, buffer.as_mut_ptr() as u32, buffer.len() as u32);
            if read == WOULD_BLOCK {
                polkadot_host_0_1_core_yield();
                continue;
            }
            if read == 0 {
                // EOF: the input stream was closed and fully drained.
                polkadot_host_0_1_core_exit(0);
            }
            if read < 0 {
                polkadot_host_0_1_core_exit(1);
            }
            for byte in &mut buffer[..read as usize] {
                *byte = byte.to_ascii_uppercase();
            }
            let written =
                polkadot_host_0_1_tty_write(tty, buffer.as_ptr() as u32, read as u32);
            if written != read {
                polkadot_host_0_1_core_exit(1);
            }
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
