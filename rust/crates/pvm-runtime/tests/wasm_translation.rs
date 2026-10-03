/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Browser-compatible compiler output under the application runtime's limits.

use pvm_runtime::{
    MAX_GUEST_HEAP_BYTES, MAX_GUEST_RW_DATA_BYTES, MAX_GUEST_STACK_BYTES, MAX_PROGRAM_BYTES,
};
use pvm_wasm_compiler::{translate, Limits};

#[test]
fn framebuffer_fixture_translates_to_valid_wasm() {
    let program = include_bytes!("fixtures/framebuffer-test.polkavm");
    let wasm = translate(
        program,
        Limits {
            max_program_bytes: MAX_PROGRAM_BYTES,
            max_rw_data_bytes: MAX_GUEST_RW_DATA_BYTES,
            max_stack_bytes: MAX_GUEST_STACK_BYTES,
            max_heap_bytes: MAX_GUEST_HEAP_BYTES,
        },
    )
    .expect("translate framebuffer fixture");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("validate translated framebuffer fixture");
    let memory_count = wasmparser::Parser::new(0)
        .parse_all(&wasm)
        .filter_map(
            |payload| match payload.expect("parse translated framebuffer fixture") {
                wasmparser::Payload::MemorySection(section) => Some(section.count()),
                _ => None,
            },
        )
        .sum::<u32>();
    assert_eq!(
        memory_count, 1,
        "translated guests must not require the WebAssembly multi-memory proposal"
    );
}
