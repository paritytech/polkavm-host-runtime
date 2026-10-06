/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Translation coverage for the host runtime's framebuffer guest fixture.

use polkavm_host_runtime::{
    MAX_GUEST_HEAP_BYTES, MAX_GUEST_RW_DATA_BYTES, MAX_GUEST_STACK_BYTES, MAX_PROGRAM_BYTES,
};
use polkavm_wasm_compiler::{translate, translate_partitioned, Limits};
use wasmi::{Engine, Linker, Module, Store};

#[test]
fn framebuffer_fixture_translates_to_valid_wasm() {
    let program = include_bytes!("fixtures/framebuffer-test.polkavm");
    let limits = Limits {
        max_program_bytes: MAX_PROGRAM_BYTES,
        max_rw_data_bytes: MAX_GUEST_RW_DATA_BYTES,
        max_stack_bytes: MAX_GUEST_STACK_BYTES,
        max_heap_bytes: MAX_GUEST_HEAP_BYTES,
    };
    for compile in [translate, translate_partitioned] {
        let wasm = compile(program, limits).expect("translate framebuffer fixture");
        wasmparser::Validator::new()
            .validate_all(&wasm)
            .expect("validate translated framebuffer fixture");
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm[..]).expect("compile framebuffer fixture");
        let mut store = Store::new(&engine, ());
        let mut linker = Linker::new(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate framebuffer fixture")
            .start(&mut store)
            .expect("start framebuffer fixture");
        linker
            .instance(&mut store, "pvm", instance)
            .expect("link framebuffer shared state");
        let mut memory_count = 0;
        for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
            match payload.expect("parse translated framebuffer fixture") {
                wasmparser::Payload::MemorySection(section) => memory_count += section.count(),
                wasmparser::Payload::CustomSection(section)
                    if section.name() == "epoca.pvm.code-part" =>
                {
                    wasmparser::Validator::new()
                        .validate_all(section.data())
                        .expect("validate translated framebuffer code part");
                    let part = Module::new(&engine, section.data())
                        .expect("compile framebuffer code part");
                    linker
                        .instantiate(&mut store, &part)
                        .expect("instantiate framebuffer code part")
                        .start(&mut store)
                        .expect("start framebuffer code part");
                }
                _ => {}
            }
        }
        assert_eq!(
            memory_count, 1,
            "translated guests must not require the WebAssembly multi-memory proposal"
        );
    }
}
