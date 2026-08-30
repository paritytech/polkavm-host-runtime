/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use pvm_runtime::{PresentationProfile, Runtime};
use std::collections::HashMap;

const PROGRAM: &[u8] = include_bytes!("fixtures/truapi-roundtrip.polkavm");
const REQUEST: &[u8] = b"truapi-conformance-request-v1";
const RESPONSE: &[u8] = b"truapi-conformance-response-v1";
const SUCCESS: &[u8] = b"truapi-roundtrip-ok";

#[test]
fn fixture_imports_the_v1_truapi_transport() {
    let blob = ProgramBlob::parse(PROGRAM.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();
    assert!(imports.iter().any(|symbol| symbol == b"host_truapi_send"));
    assert!(imports.iter().any(|symbol| symbol == b"host_truapi_poll"));
}

#[test]
fn native_runtime_roundtrips_an_opaque_truapi_frame() {
    let mut runtime = Runtime::new(
        PROGRAM,
        HashMap::new(),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
    )
    .expect("create runtime");

    runtime.init().expect("initialize guest");
    assert_eq!(runtime.take_truapi_request().as_deref(), Some(REQUEST));
    assert!(runtime.take_truapi_request().is_none());

    runtime
        .send_truapi_response(RESPONSE.to_vec())
        .expect("queue response");
    runtime.update().expect("deliver response");
    assert_eq!(runtime.take_save().as_deref(), Some(SUCCESS));
    assert!(runtime.take_save().is_none());
}
