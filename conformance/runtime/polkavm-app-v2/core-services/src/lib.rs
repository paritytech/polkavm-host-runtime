/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]
#![allow(static_mut_refs)]

const YEAR_2020_NS: u64 = 1_577_836_800_000_000_000;
const STATUS_INVALID: i32 = -3;
const STATUS_LIMIT: i32 = -6;
const SUCCESS: &[u8] = b"application-core-services-ok";

static mut FIRST_RANDOM: [u8; 32] = [0; 32];
static mut SECOND_RANDOM: [u8; 32] = [0; 32];

#[polkavm_derive::polkavm_import]
extern "C" {
    fn polkadot_host_0_1_core_clock_wall(destination: u32) -> i32;
    fn polkadot_host_0_1_core_random(destination: u32, length: u32) -> i32;
    fn host_save_submit(pointer: u32, length: u32) -> u32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    let mut wall = 0u64;
    assert_eq!(
        unsafe { polkadot_host_0_1_core_clock_wall(&mut wall as *mut u64 as u32) },
        0
    );
    assert!(wall >= YEAR_2020_NS);

    assert_eq!(
        unsafe {
            polkadot_host_0_1_core_random(
                FIRST_RANDOM.as_mut_ptr() as u32,
                FIRST_RANDOM.len() as u32,
            )
        },
        0
    );
    assert_eq!(
        unsafe {
            polkadot_host_0_1_core_random(
                SECOND_RANDOM.as_mut_ptr() as u32,
                SECOND_RANDOM.len() as u32,
            )
        },
        0
    );
    assert!(unsafe { FIRST_RANDOM != SECOND_RANDOM });
    assert!(unsafe { FIRST_RANDOM.iter().any(|byte| *byte != 0) });
    assert_eq!(unsafe { polkadot_host_0_1_core_random(0, 0) }, STATUS_INVALID);
    assert_eq!(
        unsafe { polkadot_host_0_1_core_random(FIRST_RANDOM.as_mut_ptr() as u32, 4097) },
        STATUS_LIMIT
    );
    assert_eq!(
        unsafe { host_save_submit(SUCCESS.as_ptr() as u32, SUCCESS.len() as u32) },
        0
    );
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
