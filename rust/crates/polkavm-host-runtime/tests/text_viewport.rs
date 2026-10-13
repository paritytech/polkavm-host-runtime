/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm_host_runtime::ui_wire::{
    decode_ui_output_with, UiCursorIcon, UiOutputCommand, UiOutputDecodeOptions, UiTextRun,
    TEXT_VIEWPORT_RUN_BOLD,
};
use polkavm_host_runtime::{BackendKind, PresentationProfile, Runtime};
use std::collections::HashMap;

const PROGRAM: &[u8] = include_bytes!("fixtures/text-viewport.polkavm");

fn runtime() -> Runtime {
    Runtime::new_with_backend(
        PROGRAM,
        HashMap::new(),
        PresentationProfile::Tri2d,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .expect("create runtime")
}

#[test]
fn host_without_a_renderer_rejects_the_viewport_and_keeps_the_fallback() {
    let mut runtime = runtime();
    runtime.init().expect("init");
    runtime.update().expect("fallback submit succeeds");
    let frame = runtime.take_ui_output().expect("fallback output queued");
    let output = decode_ui_output_with(&frame.bytes, UiOutputDecodeOptions::default())
        .expect("fallback is a plain output");
    assert_eq!(output.descriptor().cursor_icon, UiCursorIcon::Text);
    assert_eq!(output.command_count(), 0);
}

#[test]
fn host_with_a_renderer_accepts_the_viewport_rows() {
    let mut runtime = runtime();
    runtime.set_text_viewport_enabled(true);
    runtime.init().expect("init");
    runtime.update().expect("viewport submit succeeds");
    let frame = runtime.take_ui_output().expect("viewport output queued");
    let options = UiOutputDecodeOptions {
        text_viewport: true,
    };
    let output = decode_ui_output_with(&frame.bytes, options).expect("viewport output valid");
    let Some(UiOutputCommand::TextViewport(viewport)) = output.commands().next() else {
        panic!("expected the text viewport command");
    };
    let descriptor = viewport.descriptor();
    assert_eq!((descriptor.first_row, descriptor.row_count), (10, 2));
    assert_eq!(descriptor.cursor, Some((11, 3)));
    assert_eq!(descriptor.line_height, 20.0);
    let rows: Vec<_> = viewport.rows().collect();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].row, rows[0].text), (10, "let x = 1;"));
    assert_eq!(
        rows[0].runs().next(),
        Some(UiTextRun {
            len: 3,
            rgba: 0xff0000ff,
            flags: TEXT_VIEWPORT_RUN_BOLD,
        })
    );
    assert_eq!((rows[1].row, rows[1].text), (11, ""));
}
