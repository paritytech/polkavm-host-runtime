/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */
#![no_std]

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_args(pointer: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_core_exit(status: i32);
    fn polkadot_host_0_1_core_yield();
    fn polkadot_host_0_1_tty_read(handle: u32, pointer: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_tty_write(handle: u32, pointer: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_open(path: u32, length: u32, flags: u32) -> i32;
    fn polkadot_host_0_1_fs_read(handle: u32, pointer: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_fs_write(handle: u32, pointer: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_close(handle: u32) -> i32;
    fn polkadot_host_0_1_fs_remove(path: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_mkdir(path: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_rmdir(path: u32, length: u32) -> i32;
    fn polkadot_host_0_1_fs_rename(old: u32, old_len: u32, new: u32, new_len: u32) -> i32;
    fn polkadot_host_0_1_fs_metadata(path: u32, length: u32, pointer: u32) -> i32;
    fn polkadot_host_0_1_fs_fstat(handle: u32, pointer: u32) -> i32;
    fn polkadot_host_0_1_fs_list_directory(
        path: u32,
        length: u32,
        pointer: u32,
        capacity: u32,
    ) -> i32;
    fn polkadot_host_0_1_process_run(
        package: u32,
        package_len: u32,
        args: u32,
        args_len: u32,
    ) -> i32;
}

const DIR: &[u8] = b"/home/repo";
const EMPTY: &[u8] = b"/home/repo/empty";
const RECORD: &[u8] = b"/home/repo/record";
const TEMP: &[u8] = b"/home/repo/record.lock";
const LOCK: &[u8] = b"/home/repo/held.lock";

fn check(value: bool, status: i32) {
    if !value {
        exit(status);
    }
}
fn exit(status: i32) -> ! {
    unsafe {
        polkadot_host_0_1_core_exit(status);
    }
    loop {
        core::hint::spin_loop();
    }
}
unsafe fn open(path: &[u8], flags: u32) -> i32 {
    polkadot_host_0_1_fs_open(path.as_ptr() as u32, path.len() as u32, flags)
}
unsafe fn metadata(path: &[u8]) -> [u8; 24] {
    let mut bytes = [0u8; 24];
    check(
        polkadot_host_0_1_fs_metadata(
            path.as_ptr() as u32,
            path.len() as u32,
            bytes.as_mut_ptr() as u32,
        ) == 0,
        70,
    );
    bytes
}
unsafe fn write(path: &[u8], bytes: &[u8]) {
    let handle = open(path, 2 | 4 | 8);
    check(handle >= 16, 71);
    check(
        polkadot_host_0_1_fs_write(handle as u32, bytes.as_ptr() as u32, bytes.len() as u32)
            == bytes.len() as i32,
        72,
    );
    check(polkadot_host_0_1_fs_close(handle as u32) == 0, 73);
}
unsafe fn expect(path: &[u8], expected: &[u8]) {
    let handle = open(path, 1);
    check(handle >= 16, 74);
    let mut bytes = [0u8; 16];
    let length =
        polkadot_host_0_1_fs_read(handle as u32, bytes.as_mut_ptr() as u32, bytes.len() as u32);
    check(
        length == expected.len() as i32 && &bytes[..expected.len()] == expected,
        75,
    );
    check(polkadot_host_0_1_fs_close(handle as u32) == 0, 76);
}
unsafe fn announce(text: &[u8]) {
    check(
        polkadot_host_0_1_tty_write(1, text.as_ptr() as u32, text.len() as u32)
            == text.len() as i32,
        77,
    );
}
unsafe fn input() -> u8 {
    let mut byte = [0];
    loop {
        let count = polkadot_host_0_1_tty_read(1, byte.as_mut_ptr() as u32, 1);
        if count == 1 {
            return byte[0];
        }
        check(count == -1, 78);
        polkadot_host_0_1_core_yield();
    }
}
unsafe fn rename(old: &[u8], new: &[u8]) -> i32 {
    polkadot_host_0_1_fs_rename(
        old.as_ptr() as u32,
        old.len() as u32,
        new.as_ptr() as u32,
        new.len() as u32,
    )
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    unsafe {
        let mut args = [0u8; 128];
        let length = polkadot_host_0_1_core_args(args.as_mut_ptr() as u32, args.len() as u32);
        check(length >= 4, 10);
        if args[..length as usize]
            .windows(5)
            .any(|word| word == b"child")
        {
            check(open(LOCK, 2 | 4 | 8 | 16) == -7, 11);
            expect(RECORD, b"old");
            check(rename(TEMP, RECORD) == -5, 12);
            expect(RECORD, b"old");
            exit(27);
        }
        let mut restored_directory = [0u8; 24];
        let restored = polkadot_host_0_1_fs_metadata(
            DIR.as_ptr() as u32,
            DIR.len() as u32,
            restored_directory.as_mut_ptr() as u32,
        ) == 0;
        if args[..length as usize]
            .windows(5)
            .any(|word| word == b"check") || restored
        {
            check(metadata(EMPTY)[..4] == 2u32.to_le_bytes(), 13);
            expect(RECORD, b"new");
            expect(TEMP, b"candidate");
            check(
                polkadot_host_0_1_fs_remove(TEMP.as_ptr() as u32, TEMP.len() as u32) == 0,
                14,
            );
            announce(b"fs:restored");
            exit(0);
        }
        check(
            polkadot_host_0_1_fs_mkdir(DIR.as_ptr() as u32, DIR.len() as u32) == 0,
            15,
        );
        check(
            polkadot_host_0_1_fs_mkdir(EMPTY.as_ptr() as u32, EMPTY.len() as u32) == 0,
            16,
        );
        check(
            polkadot_host_0_1_fs_mkdir(EMPTY.as_ptr() as u32, EMPTY.len() as u32) == -7,
            17,
        );
        check(
            polkadot_host_0_1_fs_rmdir(DIR.as_ptr() as u32, DIR.len() as u32) == -10,
            18,
        );
        write(RECORD, b"old");
        write(TEMP, b"candidate");
        let locked = open(LOCK, 2 | 4 | 16);
        let destination = open(RECORD, 1);
        check(locked >= 16 && destination >= 16, 19);
        let mut by_handle = [0u8; 24];
        check(
            polkadot_host_0_1_fs_fstat(destination as u32, by_handle.as_mut_ptr() as u32) == 0,
            20,
        );
        check(by_handle == metadata(RECORD), 21);
        let child = b"fs-child";
        let child_args = b"\x01\x00\x00\x00\x05\x00\x00\x00child";
        check(
            polkadot_host_0_1_process_run(
                child.as_ptr() as u32,
                child.len() as u32,
                child_args.as_ptr() as u32,
                child_args.len() as u32,
            ) == 27,
            22,
        );
        check(
            polkadot_host_0_1_fs_close(locked as u32) == 0
                && polkadot_host_0_1_fs_close(destination as u32) == 0,
            23,
        );
        check(
            polkadot_host_0_1_fs_remove(LOCK.as_ptr() as u32, LOCK.len() as u32) == 0,
            24,
        );
        let before = metadata(RECORD);
        write(RECORD, b"new");
        let after = metadata(RECORD);
        check(before[16..] == after[16..], 25);
        check(
            u64::from_le_bytes(after[8..16].try_into().unwrap())
                > u64::from_le_bytes(before[8..16].try_into().unwrap()),
            26,
        );
        let mut listing = [0xa5u8; 128];
        let needed = polkadot_host_0_1_fs_list_directory(
            DIR.as_ptr() as u32,
            DIR.len() as u32,
            listing.as_mut_ptr() as u32,
            1,
        );
        check(needed < -10 && listing == [0xa5; 128], 28);
        let size = polkadot_host_0_1_fs_list_directory(
            DIR.as_ptr() as u32,
            DIR.len() as u32,
            listing.as_mut_ptr() as u32,
            listing.len() as u32,
        );
        let expected = b"\x03\x00\x00\x00\x05\x00\x00\x00empty\x02\x00\x00\x00\x06\x00\x00\x00record\x01\x00\x00\x00\x0b\x00\x00\x00record.lock\x01\x00\x00\x00";
        check(
            size == expected.len() as i32 && &listing[..expected.len()] == expected,
            29,
        );
        announce(b"fs:ready");
        if input() == b'c' {
            let pending = open(TEMP, 2);
            check(pending >= 16, 30);
            announce(b"fs:cancel");
            let _ = input();
            exit(31);
        }
        check(rename(TEMP, RECORD) == 0, 32);
        expect(RECORD, b"candidate");
        announce(b"fs:published");
        exit(0);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
