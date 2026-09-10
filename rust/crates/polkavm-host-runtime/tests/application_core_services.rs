/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm_host_runtime::{BackendKind, PresentationProfile, Runtime};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const PROGRAM: &[u8] = include_bytes!("fixtures/application-core-services.polkavm");

#[test]
fn native_application_reads_monotonic_wall_clock_and_secure_random() {
    let mut previous_random = None;
    for _ in 0..2 {
        let mut runtime = Runtime::new_with_backend(
            PROGRAM,
            HashMap::new(),
            PresentationProfile::Tri2d,
            false,
            10_000_000,
            BackendKind::Interpreter,
        )
        .expect("create runtime");

        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        runtime.init().expect("initialize guest");
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let output = runtime.take_save().expect("core-services output");
        assert_eq!(output.len(), 56);
        let monotonic_first = u64::from_le_bytes(output[0..8].try_into().unwrap());
        let monotonic_second = u64::from_le_bytes(output[8..16].try_into().unwrap());
        let wall = u64::from_le_bytes(output[16..24].try_into().unwrap());
        assert!(monotonic_second >= monotonic_first);
        assert!((before..=after).contains(&u128::from(wall)));
        for status in output[24..40].chunks_exact(4) {
            assert_eq!(i32::from_le_bytes(status.try_into().unwrap()), 0);
        }
        let random: [u8; 16] = output[40..56].try_into().unwrap();
        assert_ne!(random, [0; 16]);
        assert_ne!(Some(random), previous_random);
        previous_random = Some(random);
    }
}
