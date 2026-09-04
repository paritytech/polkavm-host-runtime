/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Conformance driver for the `polkadot-host-computer/0.1` workspace
//! contract. Spawns the Host-registered `pane` package as an independently
//! supervised child, exchanges terminal bytes with it, resizes it, verifies
//! nested-capability denial, persistence hand-off, exit reporting, and every
//! error path. Exits 0 only when every check passes.
//!
//! When the Host has not granted `host.workspace`, the first probe observes
//! `DENIED` and the driver exits with status 41.

#![no_std]

const WOULD_BLOCK: i32 = -1;
const BAD_HANDLE: i32 = -2;
const INVALID: i32 = -3;
const NOT_FOUND: i32 = -4;
const DENIED: i32 = -5;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_exit(status: i32);
    fn polkadot_host_0_1_tty_current() -> u32;
    fn polkadot_host_0_1_tty_write(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_workspace_spawn(
        package: u32,
        package_length: u32,
        arguments: u32,
        arguments_length: u32,
        columns: u32,
        rows: u32,
    ) -> i32;
    fn polkadot_host_0_1_workspace_send_input(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_workspace_read(handle: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_workspace_resize(handle: u32, columns: u32, rows: u32) -> i32;
    fn polkadot_host_0_1_workspace_wait(handle: u32) -> i32;
    fn polkadot_host_0_1_workspace_close(handle: u32) -> i32;
}

fn fail(code: i32) -> ! {
    unsafe {
        polkadot_host_0_1_core_exit(code);
    }
    loop {
        core::hint::spin_loop();
    }
}

unsafe fn spawn(package: &[u8], columns: u32, rows: u32) -> i32 {
    polkadot_host_0_1_workspace_spawn(
        package.as_ptr() as u32,
        package.len() as u32,
        0,
        0,
        columns,
        rows,
    )
}

unsafe fn send(handle: u32, bytes: &[u8]) -> i32 {
    polkadot_host_0_1_workspace_send_input(handle, bytes.as_ptr() as u32, bytes.len() as u32)
}

/// Reads exactly `expected` from the child surface; any other bytes,
/// `WOULD_BLOCK`, or EOF fails with `code`. The supervisor drives the child
/// inside the read hostcall, so expected output is available immediately.
unsafe fn expect_output(handle: u32, expected: &[u8], code: i32) {
    let mut received = [0u8; 64];
    let mut length = 0usize;
    while length < expected.len() {
        let read = polkadot_host_0_1_workspace_read(
            handle,
            received.as_mut_ptr().wrapping_add(length) as u32,
            (expected.len() - length) as u32,
        );
        if read <= 0 {
            fail(code);
        }
        length += read as usize;
    }
    if &received[..length] != expected {
        fail(code);
    }
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    unsafe {
        let tty = polkadot_host_0_1_tty_current();

        // Capability gate: without the workspace grant every operation is
        // denied; report that distinctly for the host-side denial test.
        let probe = polkadot_host_0_1_workspace_close(99);
        if probe == DENIED {
            fail(41);
        }
        // Operations against unknown handles must be rejected.
        if probe != BAD_HANDLE {
            fail(10);
        }
        if polkadot_host_0_1_workspace_wait(99) != BAD_HANDLE {
            fail(11);
        }
        let mut scratch = [0u8; 4];
        if polkadot_host_0_1_workspace_read(99, scratch.as_mut_ptr() as u32, 4) != BAD_HANDLE {
            fail(12);
        }
        if send(99, b"x") != BAD_HANDLE {
            fail(13);
        }
        if polkadot_host_0_1_workspace_resize(99, 10, 10) != BAD_HANDLE {
            fail(14);
        }

        // Unknown packages and invalid geometry are rejected without effects.
        if spawn(b"no-such-package", 40, 12) != NOT_FOUND {
            fail(15);
        }
        if spawn(b"pane", 0, 12) != INVALID {
            fail(16);
        }
        if spawn(b"pane", 40, 2_000) != INVALID {
            fail(17);
        }

        let handle = spawn(b"pane", 40, 12);
        if handle <= 0 {
            fail(18);
        }
        let handle = handle as u32;

        // The child announces itself on first drive.
        expect_output(handle, b"pane:ready", 19);
        // A live child reports WOULD_BLOCK from wait.
        if polkadot_host_0_1_workspace_wait(handle) != WOULD_BLOCK {
            fail(20);
        }

        // Byte roundtrip: the pane toggles the case bit.
        if send(handle, b"ab") != 2 {
            fail(21);
        }
        expect_output(handle, b"AB", 22);

        // Resize is observable through the child's own tty_get_size.
        if polkadot_host_0_1_workspace_resize(handle, 20, 5) != 0 {
            fail(23);
        }
        if send(handle, b"s") != 1 {
            fail(24);
        }
        expect_output(handle, b"20x5", 25);
        if polkadot_host_0_1_workspace_resize(handle, 0, 5) != INVALID {
            fail(26);
        }

        // Nested computers are never granted host.workspace.
        if send(handle, b"n") != 1 {
            fail(27);
        }
        expect_output(handle, b"n:denied", 28);

        // Persistence flows child -> parent /home.
        if send(handle, b"w") != 1 {
            fail(29);
        }
        expect_output(handle, b"w:ok", 30);

        // Exit is reported through wait; the handle stays valid for
        // draining and is reclaimed only by close.
        if send(handle, b"q") != 1 {
            fail(31);
        }
        if polkadot_host_0_1_workspace_wait(handle) != 7 {
            fail(32);
        }
        if polkadot_host_0_1_workspace_read(handle, scratch.as_mut_ptr() as u32, 4) != 0 {
            fail(33);
        }
        if send(handle, b"x") != INVALID {
            fail(34);
        }
        if polkadot_host_0_1_workspace_close(handle) != 0 {
            fail(35);
        }
        if polkadot_host_0_1_workspace_close(handle) != BAD_HANDLE {
            fail(36);
        }

        let done = b"workspace:ok";
        polkadot_host_0_1_tty_write(tty, done.as_ptr() as u32, done.len() as u32);
        polkadot_host_0_1_core_exit(0);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
