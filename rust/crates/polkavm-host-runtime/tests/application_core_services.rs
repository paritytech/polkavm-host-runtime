/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use polkavm_host_runtime::{BackendKind, PresentationProfile, Runtime};
use std::collections::HashMap;

const PROGRAM: &[u8] = include_bytes!("fixtures/application-core-services.polkavm");
const SUCCESS: &[u8] = b"application-core-services-ok";

#[test]
fn fixture_imports_wall_clock_and_random_core_services() {
    let blob = ProgramBlob::parse(PROGRAM.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();

    assert!(imports
        .iter()
        .any(|symbol| symbol == b"polkadot_host_0_1_core_clock_wall"));
    assert!(imports
        .iter()
        .any(|symbol| symbol == b"polkadot_host_0_1_core_random"));
}

#[test]
fn native_application_reads_wall_clock_and_secure_random() {
    let mut runtime = Runtime::new_with_backend(
        PROGRAM,
        HashMap::new(),
        PresentationProfile::Tri2d,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .expect("create runtime");

    runtime.init().expect("initialize guest");
    assert_eq!(runtime.take_save().as_deref(), Some(SUCCESS));
}
