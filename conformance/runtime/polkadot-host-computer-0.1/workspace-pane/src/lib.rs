/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Conformance pane for the `polkadot-host-computer/0.1` workspace contract.
//! Runs as an independently supervised workspace child: announces itself,
//! then serves single-byte commands on its own terminal.
//!
//! - `s`: report the current terminal size as `<columns>x<rows>`
//! - `w`: persist `/home/pane.txt` and reply `w:ok`
//! - `n`: probe a workspace call, reply `n:denied` when it is refused
//! - `q`: exit with status 7
//! - any other byte: echo it with the case bit toggled

#![no_std]

const WOULD_BLOCK: i32 = -1;
const DENIED: i32 = -5;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_yield();
    fn polkadot_host_0_1_core_exit(status: i32);
    fn polkadot_host_0_1_tty_current() -> u32;
    fn polkadot_host_0_1_tty_read(handle: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_tty_write(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_tty_get_size(handle: u32, record: u32) -> i32;
    fn polkadot_host_0_1_fs_open(path: u32, path_length: u32, flags: u32) -> i32;
    fn polkadot_host_0_1_fs_write(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_read(handle: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_fs_close(handle: u32) -> i32;
    fn polkadot_host_0_1_workspace_close(handle: u32) -> i32;
    fn polkadot_host_0_1_process_run(
        package: u32,
        package_length: u32,
        arguments: u32,
        arguments_length: u32,
    ) -> i32;
}

fn fail(code: i32) -> ! {
    unsafe {
        polkadot_host_0_1_core_exit(code);
    }
    loop {
        core::hint::spin_loop();
    }
}

unsafe fn write_all(tty: u32, bytes: &[u8]) {
    let mut offset = 0usize;
    while offset < bytes.len() {
        let written = polkadot_host_0_1_tty_write(
            tty,
            bytes.as_ptr().wrapping_add(offset) as u32,
            (bytes.len() - offset) as u32,
        );
        if written <= 0 {
            fail(30);
        }
        offset += written as usize;
    }
}

/// Appends `value` as decimal digits; returns the new length.
fn push_decimal(buffer: &mut [u8], mut length: usize, value: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut count = 0usize;
    let mut rest = value;
    loop {
        digits[count] = b'0' + (rest % 10) as u8;
        rest /= 10;
        count += 1;
        if rest == 0 {
            break;
        }
    }
    while count > 0 {
        count -= 1;
        buffer[length] = digits[count];
        length += 1;
    }
    length
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    unsafe {
        let tty = polkadot_host_0_1_tty_current();
        write_all(tty, b"pane:ready");

        loop {
            let mut byte = [0u8; 1];
            let read = polkadot_host_0_1_tty_read(tty, byte.as_mut_ptr() as u32, 1);
            if read == WOULD_BLOCK {
                polkadot_host_0_1_core_yield();
                continue;
            }
            if read == 0 {
                // Input closed by the workspace: exit cleanly.
                polkadot_host_0_1_core_exit(0);
            }
            if read != 1 {
                fail(31);
            }
            match byte[0] {
                b'q' => polkadot_host_0_1_core_exit(7),
                b's' => {
                    let mut record = [0u8; 8];
                    if polkadot_host_0_1_tty_get_size(tty, record.as_mut_ptr() as u32) != 0 {
                        fail(32);
                    }
                    let columns = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
                    let rows = u32::from_le_bytes([record[4], record[5], record[6], record[7]]);
                    let mut reply = [0u8; 24];
                    let mut length = push_decimal(&mut reply, 0, columns);
                    reply[length] = b'x';
                    length += 1;
                    length = push_decimal(&mut reply, length, rows);
                    write_all(tty, &reply[..length]);
                }
                b'w' => {
                    let path = b"/home/pane.txt";
                    // FS_OPEN_WRITE | FS_OPEN_CREATE
                    let handle =
                        polkadot_host_0_1_fs_open(path.as_ptr() as u32, path.len() as u32, 2 | 4);
                    if handle < 0 {
                        fail(33);
                    }
                    let contents = b"from-pane";
                    if polkadot_host_0_1_fs_write(
                        handle as u32,
                        contents.as_ptr() as u32,
                        contents.len() as u32,
                    ) != contents.len() as i32
                    {
                        fail(34);
                    }
                    if polkadot_host_0_1_fs_close(handle as u32) != 0 {
                        fail(35);
                    }
                    write_all(tty, b"w:ok");
                }
                b'n' => {
                    // A workspace child is never granted host.workspace.
                    if polkadot_host_0_1_workspace_close(1) != DENIED {
                        fail(36);
                    }
                    write_all(tty, b"n:denied");
                }
                b'p' => {
                    // Runs a package inside this pane's own foreground
                    // stack. With open resolution enabled and the package
                    // unregistered, the request suspends the whole tree
                    // until the embedder provides or rejects it.
                    let package = b"extra";
                    let status = polkadot_host_0_1_process_run(
                        package.as_ptr() as u32,
                        package.len() as u32,
                        0,
                        0,
                    );
                    let mut reply = [0u8; 16];
                    reply[0] = b'p';
                    reply[1] = b':';
                    let mut length = 2;
                    if status < 0 {
                        reply[length] = b'-';
                        length += 1;
                    }
                    let length = push_decimal(&mut reply, length, status.unsigned_abs());
                    write_all(tty, &reply[..length]);
                }
                b'r' => {
                    // Reports /home/seed.txt: the Host mounts it only after
                    // this pane spawned, so a hit proves live parent->child
                    // mount propagation (open-resolution seed files).
                    let path = b"/home/seed.txt";
                    let handle = polkadot_host_0_1_fs_open(
                        path.as_ptr() as u32,
                        path.len() as u32,
                        1,
                    );
                    if handle < 0 {
                        write_all(tty, b"r:missing");
                    } else {
                        let mut contents = [0u8; 16];
                        let read = polkadot_host_0_1_fs_read(
                            handle as u32,
                            contents.as_mut_ptr() as u32,
                            contents.len() as u32,
                        );
                        polkadot_host_0_1_fs_close(handle as u32);
                        if read < 0 {
                            fail(37);
                        }
                        write_all(tty, b"r:");
                        write_all(tty, &contents[..read as usize]);
                    }
                }
                other => {
                    let toggled = [other ^ 0x20];
                    write_all(tty, &toggled);
                }
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
