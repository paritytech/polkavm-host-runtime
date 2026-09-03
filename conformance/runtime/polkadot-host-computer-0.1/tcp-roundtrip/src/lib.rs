/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Conformance guest for the `polkadot-host-computer/0.1` outbound TCP
//! capability. Connects to the address named by the NET_TARGET environment
//! entry, sends `ping\n`, expects the transformed reply `PING\n`, and exits
//! 0. Exits 21 when the network capability is denied, and other distinct
//! codes on contract violations.

#![no_std]
#![allow(static_mut_refs)]

const WOULD_BLOCK: i32 = -1;
const DENIED: i32 = -5;
const REQUEST: &[u8] = b"ping\n";
const EXPECTED: &[u8] = b"PING\n";

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_environment(pointer: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_core_yield();
    fn polkadot_host_0_1_core_exit(status: i32);
    fn polkadot_host_0_1_net_tcp_connect(address: u32, address_length: u32) -> i32;
    fn polkadot_host_0_1_net_read(handle: u32, destination: u32, capacity: u32) -> i32;
    fn polkadot_host_0_1_net_write(handle: u32, source: u32, length: u32) -> i32;
    fn polkadot_host_0_1_net_close(handle: u32) -> i32;
}

static mut ENVIRONMENT: [u8; 4096] = [0; 4096];

fn fail(code: i32) -> ! {
    unsafe {
        polkadot_host_0_1_core_exit(code);
    }
    loop {
        core::hint::spin_loop();
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice = bytes.get(offset..end)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Returns the NET_TARGET value range within the environment record.
fn find_target(record: &[u8]) -> Option<(usize, usize)> {
    let count = read_u32(record, 0)? as usize;
    let mut offset = 4usize;
    // The count is the number of key/value entries.
    for _ in 0..count {
        let key_length = read_u32(record, offset)? as usize;
        offset += 4;
        let key = record.get(offset..offset + key_length)?;
        offset += key_length;
        let value_length = read_u32(record, offset)? as usize;
        offset += 4;
        if offset + value_length > record.len() {
            return None;
        }
        if key == b"NET_TARGET" {
            return Some((offset, value_length));
        }
        offset += value_length;
    }
    None
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    unsafe {
        let length = polkadot_host_0_1_core_environment(
            ENVIRONMENT.as_mut_ptr() as u32,
            ENVIRONMENT.len() as u32,
        );
        if length <= 0 {
            fail(10);
        }
        let Some((offset, target_length)) = find_target(&ENVIRONMENT[..length as usize]) else {
            fail(11);
        };

        let handle = polkadot_host_0_1_net_tcp_connect(
            ENVIRONMENT.as_ptr() as u32 + offset as u32,
            target_length as u32,
        );
        if handle == DENIED {
            fail(21);
        }
        if handle <= 0 {
            fail(12);
        }
        let handle = handle as u32;

        let mut sent = 0usize;
        while sent < REQUEST.len() {
            let written = polkadot_host_0_1_net_write(
                handle,
                REQUEST.as_ptr().wrapping_add(sent) as u32,
                (REQUEST.len() - sent) as u32,
            );
            if written == WOULD_BLOCK {
                polkadot_host_0_1_core_yield();
                continue;
            }
            if written <= 0 {
                fail(13);
            }
            sent += written as usize;
        }

        let mut received = [0u8; 16];
        let mut length = 0usize;
        while length < EXPECTED.len() {
            let read = polkadot_host_0_1_net_read(
                handle,
                received.as_mut_ptr() as u32 + length as u32,
                (received.len() - length) as u32,
            );
            if read == WOULD_BLOCK {
                polkadot_host_0_1_core_yield();
                continue;
            }
            if read == 0 {
                fail(14);
            }
            if read < 0 {
                fail(15);
            }
            length += read as usize;
        }
        if &received[..EXPECTED.len()] != EXPECTED {
            fail(16);
        }

        if polkadot_host_0_1_net_close(handle) != 0 {
            fail(17);
        }
        if polkadot_host_0_1_net_close(handle) == 0 {
            fail(18);
        }
        polkadot_host_0_1_core_exit(0);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
