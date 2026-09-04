/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Conformance guest for the `polkadot-host-computer/0.1` TTY and filesystem
//! capabilities. It reads a mounted seed file, announces readiness, echoes
//! terminal input uppercased, and on `q` persists everything it received to
//! `/home/echo.txt` before exiting with status 7.

#![no_std]
#![allow(static_mut_refs)]

const SEED_PATH: &[u8] = b"/home/seed.txt";
const ECHO_PATH: &[u8] = b"/home/echo.txt";
const REMOVE_PATH: &[u8] = b"/home/remove.tmp";
const MODE_RAW: u32 = 1;
const OPEN_READ: u32 = 1;
const OPEN_WRITE: u32 = 2;
const OPEN_CREATE: u32 = 4;
const OPEN_TRUNCATE: u32 = 8;
const WOULD_BLOCK: i32 = -1;
const EXIT_SUCCESS: i32 = 7;

static mut RECEIVED: [u8; 256] = [0; 256];
static mut RECEIVED_LENGTH: usize = 0;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_yield();
    fn polkadot_host_0_1_core_exit(status: i32);
    fn polkadot_host_0_1_tty_current() -> u32;
    fn polkadot_host_0_1_tty_read(handle: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_tty_write(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_tty_get_size(handle: u32, record: u32) -> i32;
    fn polkadot_host_0_1_tty_set_mode(handle: u32, flags: u32) -> i32;
    fn polkadot_host_0_1_fs_open(path: u32, path_length: u32, flags: u32) -> i32;
    fn polkadot_host_0_1_fs_read(handle: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_fs_write(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_seek(handle: u32, offset: i32, whence: u32) -> i32;
    fn polkadot_host_0_1_fs_truncate(handle: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_stat(path: u32, path_length: u32, record: u32) -> i32;
    fn polkadot_host_0_1_fs_sync(handle: u32) -> i32;
    fn polkadot_host_0_1_fs_close(handle: u32) -> i32;
    fn polkadot_host_0_1_fs_remove(path: u32, path_length: u32) -> i32;
}

fn tty_write(handle: u32, bytes: &[u8]) {
    let written =
        unsafe { polkadot_host_0_1_tty_write(handle, bytes.as_ptr() as u32, bytes.len() as u32) };
    assert_eq!(written, bytes.len() as i32);
}

fn read_seed(buffer: &mut [u8]) -> usize {
    unsafe {
        let mut stat = [0u8; 4];
        assert_eq!(
            polkadot_host_0_1_fs_stat(
                SEED_PATH.as_ptr() as u32,
                SEED_PATH.len() as u32,
                stat.as_mut_ptr() as u32,
            ),
            0
        );
        let handle = polkadot_host_0_1_fs_open(
            SEED_PATH.as_ptr() as u32,
            SEED_PATH.len() as u32,
            OPEN_READ,
        );
        assert!(handle > 0);
        let handle = handle as u32;
        let read =
            polkadot_host_0_1_fs_read(handle, buffer.as_mut_ptr() as u32, buffer.len() as u32);
        assert!(read > 0);
        assert_eq!(u32::from_le_bytes(stat), read as u32);
        assert_eq!(polkadot_host_0_1_fs_seek(handle, 0, 0), 0);
        assert_eq!(polkadot_host_0_1_fs_close(handle), 0);
        read as usize
    }
}

fn save_and_exit() -> ! {
    unsafe {
        let handle = polkadot_host_0_1_fs_open(
            ECHO_PATH.as_ptr() as u32,
            ECHO_PATH.len() as u32,
            OPEN_READ | OPEN_WRITE | OPEN_CREATE | OPEN_TRUNCATE,
        );
        assert!(handle > 0);
        let handle = handle as u32;
        assert_eq!(polkadot_host_0_1_fs_truncate(handle, 0), 0);
        let written = polkadot_host_0_1_fs_write(
            handle,
            RECEIVED.as_ptr() as u32,
            RECEIVED_LENGTH as u32,
        );
        assert_eq!(written, RECEIVED_LENGTH as i32);
        assert_eq!(polkadot_host_0_1_fs_sync(handle), 0);
        assert_eq!(polkadot_host_0_1_fs_close(handle), 0);
        let remove_handle = polkadot_host_0_1_fs_open(
            REMOVE_PATH.as_ptr() as u32,
            REMOVE_PATH.len() as u32,
            OPEN_WRITE | OPEN_CREATE,
        );
        assert!(remove_handle > 0);
        assert_eq!(polkadot_host_0_1_fs_close(remove_handle as u32), 0);
        assert_eq!(
            polkadot_host_0_1_fs_remove(
                REMOVE_PATH.as_ptr() as u32,
                REMOVE_PATH.len() as u32,
            ),
            0
        );
        polkadot_host_0_1_core_exit(EXIT_SUCCESS);
    }
    unreachable!()
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    unsafe {
        let tty = polkadot_host_0_1_tty_current();
        assert_eq!(polkadot_host_0_1_tty_set_mode(tty, MODE_RAW), 0);
        let mut size = [0u8; 8];
        assert_eq!(
            polkadot_host_0_1_tty_get_size(tty, size.as_mut_ptr() as u32),
            0
        );
        assert_ne!(u32::from_le_bytes([size[0], size[1], size[2], size[3]]), 0);

        let mut seed = [0u8; 64];
        let seed_length = read_seed(&mut seed);
        tty_write(tty, b"ready:");
        tty_write(tty, &seed[..seed_length]);
        tty_write(tty, b"\r\n");

        loop {
            let mut buffer = [0u8; 16];
            let read =
                polkadot_host_0_1_tty_read(tty, buffer.as_mut_ptr() as u32, buffer.len() as u32);
            if read == WOULD_BLOCK {
                polkadot_host_0_1_core_yield();
                continue;
            }
            assert!(read > 0);
            for &byte in &buffer[..read as usize] {
                if byte == b'q' {
                    save_and_exit();
                }
                if RECEIVED_LENGTH < RECEIVED.len() {
                    RECEIVED[RECEIVED_LENGTH] = byte;
                    RECEIVED_LENGTH += 1;
                }
                tty_write(tty, &[byte.to_ascii_uppercase()]);
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
