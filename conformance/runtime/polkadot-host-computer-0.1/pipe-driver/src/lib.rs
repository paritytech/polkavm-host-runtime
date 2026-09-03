/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Conformance driver for the `polkadot-host-computer/0.1` process pipes.
//! Spawns the Host-registered `upper` package, streams input through it,
//! verifies the transformed output and exit status, and checks the error
//! paths (unknown package, bad pid). Exits 0 only when every check passes.

#![no_std]

const WOULD_BLOCK: i32 = -1;
const BAD_HANDLE: i32 = -2;
const NOT_FOUND: i32 = -4;
const INPUT: &[u8] = b"hello, pipes";
const EXPECTED: &[u8] = b"HELLO, PIPES";

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_yield();
    fn polkadot_host_0_1_core_exit(status: i32);
    fn polkadot_host_0_1_tty_current() -> u32;
    fn polkadot_host_0_1_tty_write(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_process_spawn(
        package: u32,
        package_length: u32,
        arguments: u32,
        arguments_length: u32,
    ) -> i32;
    fn polkadot_host_0_1_process_wait(pid: u32) -> i32;
    fn polkadot_host_0_1_pipe_read(pid: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_pipe_write(pid: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_pipe_close(pid: u32) -> i32;
}

fn fail(code: i32) -> ! {
    unsafe {
        polkadot_host_0_1_core_exit(code);
    }
    loop {
        core::hint::spin_loop();
    }
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    unsafe {
        let tty = polkadot_host_0_1_tty_current();

        // Unknown packages must be rejected without side effects.
        let missing = b"no-such-package";
        if polkadot_host_0_1_process_spawn(missing.as_ptr() as u32, missing.len() as u32, 0, 0)
            != NOT_FOUND
        {
            fail(10);
        }
        // Pipe operations against unknown pids must be rejected.
        if polkadot_host_0_1_pipe_close(99) != BAD_HANDLE {
            fail(11);
        }
        if polkadot_host_0_1_process_wait(99) != BAD_HANDLE {
            fail(12);
        }

        let package = b"upper";
        let pid =
            polkadot_host_0_1_process_spawn(package.as_ptr() as u32, package.len() as u32, 0, 0);
        if pid <= 0 {
            fail(13);
        }
        let pid = pid as u32;

        let mut offset = 0usize;
        while offset < INPUT.len() {
            let written = polkadot_host_0_1_pipe_write(
                pid,
                INPUT.as_ptr().wrapping_add(offset) as u32,
                (INPUT.len() - offset) as u32,
            );
            if written < 0 {
                fail(14);
            }
            offset += written as usize;
        }
        if polkadot_host_0_1_pipe_close(pid) != 0 {
            fail(15);
        }

        let mut received = [0u8; 64];
        let mut length = 0usize;
        loop {
            let mut chunk = [0u8; 16];
            let read =
                polkadot_host_0_1_pipe_read(pid, chunk.as_mut_ptr() as u32, chunk.len() as u32);
            if read == WOULD_BLOCK {
                polkadot_host_0_1_core_yield();
                continue;
            }
            if read == 0 {
                break;
            }
            if read < 0 || length + read as usize > received.len() {
                fail(16);
            }
            received[length..length + read as usize].copy_from_slice(&chunk[..read as usize]);
            length += read as usize;
        }
        if &received[..length] != EXPECTED {
            fail(17);
        }

        // The exited child is reaped exactly once.
        if polkadot_host_0_1_process_wait(pid) != 0 {
            fail(18);
        }
        if polkadot_host_0_1_process_wait(pid) != BAD_HANDLE {
            fail(19);
        }

        polkadot_host_0_1_tty_write(tty, EXPECTED.as_ptr() as u32, EXPECTED.len() as u32);
        polkadot_host_0_1_core_exit(0);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
