/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use pvm_runtime::{BackendKind, ComputerContext, ComputerStatus, ComputerSupervisor};

const DRIVER: &[u8] = include_bytes!("fixtures/computer-workspace-driver.polkavm");
const PANE: &[u8] = include_bytes!("fixtures/computer-workspace-pane.polkavm");

fn supervisor(program: &[u8]) -> ComputerSupervisor {
    ComputerSupervisor::new_with_backend(
        program,
        ComputerContext::default(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create supervisor")
}

#[test]
fn fixture_imports_versioned_workspace_operations() {
    let blob = ProgramBlob::parse(DRIVER.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();

    for required in [
        b"polkadot_host_0_1_workspace_spawn".as_slice(),
        b"polkadot_host_0_1_workspace_send_input".as_slice(),
        b"polkadot_host_0_1_workspace_read".as_slice(),
        b"polkadot_host_0_1_workspace_resize".as_slice(),
        b"polkadot_host_0_1_workspace_wait".as_slice(),
        b"polkadot_host_0_1_workspace_close".as_slice(),
    ] {
        assert!(
            imports.iter().any(|symbol| symbol == required),
            "missing import {}",
            String::from_utf8_lossy(required)
        );
    }
}

#[test]
fn workspace_guest_supervises_an_independent_child() {
    let mut supervisor = supervisor(DRIVER);
    supervisor.register_package("pane", PANE.to_vec()).unwrap();
    supervisor.set_workspace_enabled(true);

    // The driver asserts every contract detail internally (bad handles,
    // unknown package, invalid geometry, banner, byte roundtrip, resize
    // observability, nested denial, persistence, exit reporting, EOF after
    // drain, close-once) and exits nonzero with a distinct code on the
    // first violation.
    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Exited(0));
    assert_eq!(
        supervisor.take_terminal_output().as_deref(),
        Some(b"workspace:ok".as_slice())
    );

    // The pane's `/home` write surfaced through the parent supervisor.
    let modified = supervisor.take_modified_files();
    assert!(
        modified
            .iter()
            .any(|(path, bytes)| path == "/home/pane.txt" && bytes == b"from-pane"),
        "pane write should merge into the parent /home: {modified:?}"
    );
}

#[test]
fn workspace_operations_are_denied_without_the_grant() {
    let mut supervisor = supervisor(DRIVER);
    supervisor.register_package("pane", PANE.to_vec()).unwrap();

    // Without set_workspace_enabled the driver's first probe observes
    // DENIED and exits with its distinct gating code.
    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Exited(41));
}
