/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! File-input conformance through `tests/fixtures/file-input.polkavm`.
//!
//! The fixture is assembled by [`file_input_program`], so any Host platform
//! reproduces it byte for byte. During `init` the guest walks the
//! `descriptors` asset, a sequence of `[tag u32][length u32][payload]` records
//! ended by tag 0: tag 1 registers the payload with `host_file_register` and
//! tag 2 registers `camera-ur` with `host_input_register`. Tags 3 and 4 carry
//! no payload; their `length` indexes an earlier record, whose handle tag 3
//! triggers and tag 4 selects for cancellation. The guest saves the sixteen
//! `i32` results. Every `update` cancels the selected handle with
//! `host_input_cancel`, probes handles 1 through 4 with `host_input_status`,
//! `host_file_info`, and `host_input_read`, reads the `game/cartridge.bin`
//! asset, and saves the results followed by one [`Snapshot`].

use polkavm::Reg;
use polkavm_common::abi::MemoryMapBuilder;
use polkavm_common::program::{asm, Instruction, InstructionSetKind};
use polkavm_common::writer::ProgramBlobBuilder;
use polkavm_host_runtime::{
    ApplicationRuntime, BackendKind, FileDelivery, FileInputDelivery, FileInputSupport,
    FileRelaunch, FileSelection, MediatedInputCommand, MediatedInputStatus, PresentationProfile,
    FILE_REGISTER_DELIVERY_UNAVAILABLE, MEDIATED_INPUT_CANCEL_ACCEPTED,
    MEDIATED_INPUT_CANCEL_NOT_ACTIVE, MEDIATED_INPUT_REGISTER_INVALID,
    MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED, MEDIATED_INPUT_REGISTER_UNAVAILABLE,
};
use std::collections::HashMap;

const FIXTURE: &[u8] = include_bytes!("fixtures/file-input.polkavm");
const DESCRIPTORS_ASSET: &str = "descriptors";
const CAMERA_KIND: &str = "camera-ur";
const CAMERA_MEDIA_TYPE: &str = "x-test-payload";
const MOUNT_PATH: &str = "game/cartridge.bin";
const ENTRYPOINT: &str = "app.polkavm";
const RESULTS: usize = 16;
const PROBED_HANDLES: u32 = 4;
const INFO_BYTES: usize = 128;
const DATA_BYTES: usize = 64;
const PROBE_STRIDE: usize = 12 + INFO_BYTES + DATA_BYTES;
const ASSET_OFFSET: usize = PROBE_STRIDE * PROBED_HANDLES as usize;
const CANCEL_OFFSET: usize = ASSET_OFFSET + 4 + DATA_BYTES;
const OUTPUT_BYTES: usize = CANCEL_OFFSET + 4;
/// An update snapshot repeats the results and the cancelled handle first,
/// since a later save replaces one the Host has not taken yet.
const SNAPSHOT_HEADER_BYTES: usize = RESULTS * 4 + 4;
const DESCRIPTOR_BUFFER_BYTES: usize = 16 * 1024;

const INLINE: &str = r#"{"id":"doc","label":"Text document","extensions":[".txt"],"mimeTypes":["text/plain"],"delivery":"inline","maxBytes":16}"#;
const RELAUNCH: &str = r#"{"id":"rom","label":"Cartridge","extensions":[".bin"],"delivery":"relaunch","maxBytes":32,"mountPath":"game/cartridge.bin"}"#;

fn file_input_program() -> Vec<u8> {
    let names = [
        DESCRIPTORS_ASSET,
        CAMERA_KIND,
        CAMERA_MEDIA_TYPE,
        MOUNT_PATH,
    ];
    let ro_data = names.concat().into_bytes();
    let rw_size = 32 * 1024;
    let stack_size = 4 * 1024;
    let memory = MemoryMapBuilder::new(64 * 1024)
        .ro_data_size(ro_data.len() as u32)
        .rw_data_size(rw_size)
        .stack_size(stack_size)
        .build()
        .unwrap();
    let mut name_addresses = Vec::new();
    let mut offset = memory.ro_data_address();
    for name in names {
        name_addresses.push((offset as i32, name.len() as i32));
        offset += name.len() as u32;
    }
    let [descriptors, camera_kind, camera_media_type, mount_path] = name_addresses[..] else {
        unreachable!()
    };
    let descriptor_buffer = memory.rw_data_address() as i32;
    let results = descriptor_buffer + DESCRIPTOR_BUFFER_BYTES as i32;
    let cancel_slot = results + (RESULTS * 4) as i32;
    let output = cancel_slot + 4;

    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_ro_data_size(ro_data.len() as u32);
    builder.set_ro_data(ro_data);
    builder.set_rw_data_size(rw_size);
    builder.set_stack_size(stack_size);
    for import in [
        "host_asset_read",
        "host_file_register",
        "host_input_register",
        "host_input_trigger",
        "host_save_submit",
        "host_input_status",
        "host_file_info",
        "host_input_read",
        "host_input_cancel",
    ] {
        builder.add_import(import.as_bytes());
    }
    let [asset_read, file_register, input_register, input_trigger, save_submit, input_status, file_info, input_read, input_cancel] =
        [0, 1, 2, 3, 4, 5, 6, 7, 8];

    // Basic blocks: 0 init, 1 loop, 2 tag 2?, 3 tag 3?, 4 tag 4?, 5 file,
    // 6 input, 7 trigger, 8 cancel, 9 store, 10 done, 11 update.
    const LOOP: u32 = 1;
    const INPUT: u32 = 6;
    const TRIGGER: u32 = 7;
    const CANCEL: u32 = 8;
    const STORE: u32 = 9;
    const DONE: u32 = 10;
    let mut code: Vec<Instruction> = vec![
        asm::load_imm(Reg::A0, descriptors.0),
        asm::load_imm(Reg::A1, descriptors.1),
        asm::load_imm(Reg::A2, 0),
        asm::load_imm(Reg::A3, descriptor_buffer),
        asm::load_imm(Reg::A4, DESCRIPTOR_BUFFER_BYTES as i32 - 8),
        asm::ecalli(asset_read),
        asm::load_imm(Reg::S0, descriptor_buffer),
        asm::load_imm(Reg::S1, results),
        asm::fallthrough(),
        // LOOP
        asm::load_indirect_i32(Reg::T0, Reg::S0, 0),
        asm::load_indirect_i32(Reg::T1, Reg::S0, 4),
        asm::branch_eq_imm(Reg::T0, 0, DONE),
        asm::branch_eq_imm(Reg::T0, 2, INPUT),
        asm::branch_eq_imm(Reg::T0, 3, TRIGGER),
        asm::branch_eq_imm(Reg::T0, 4, CANCEL),
        // FILE
        asm::add_imm_32(Reg::A0, Reg::S0, 8),
        asm::move_reg(Reg::A1, Reg::T1),
        asm::ecalli(file_register),
        asm::jump(STORE),
        // INPUT
        asm::load_imm(Reg::A0, camera_kind.0),
        asm::load_imm(Reg::A1, camera_kind.1),
        asm::load_imm(Reg::A2, camera_media_type.0),
        asm::load_imm(Reg::A3, camera_media_type.1),
        asm::load_imm(Reg::A4, 16),
        asm::ecalli(input_register),
        asm::jump(STORE),
        // TRIGGER
        asm::shift_logical_left_imm_32(Reg::T2, Reg::T1, 2),
        asm::add_imm_32(Reg::T2, Reg::T2, results),
        asm::load_indirect_i32(Reg::A0, Reg::T2, 0),
        asm::ecalli(input_trigger),
        asm::load_imm(Reg::T1, 0),
        asm::jump(STORE),
        // CANCEL
        asm::shift_logical_left_imm_32(Reg::T2, Reg::T1, 2),
        asm::add_imm_32(Reg::T2, Reg::T2, results),
        asm::load_indirect_i32(Reg::A0, Reg::T2, 0),
        asm::store_u32(Reg::A0, cancel_slot),
        asm::load_imm(Reg::T1, 0),
        asm::fallthrough(),
        // STORE
        asm::store_indirect_u32(Reg::A0, Reg::S1, 0),
        asm::add_imm_32(Reg::S1, Reg::S1, 4),
        asm::add_32(Reg::S0, Reg::S0, Reg::T1),
        asm::add_imm_32(Reg::S0, Reg::S0, 8),
        asm::jump(LOOP),
        // DONE
        asm::load_imm(Reg::A0, results),
        asm::load_imm(Reg::A1, (RESULTS * 4) as i32),
        asm::ecalli(save_submit),
        asm::ret(),
    ];
    // UPDATE
    code.extend([
        asm::load_imm(Reg::T0, cancel_slot),
        asm::load_indirect_i32(Reg::A0, Reg::T0, 0),
        asm::ecalli(input_cancel),
        asm::store_u32(Reg::A0, output + CANCEL_OFFSET as i32),
    ]);
    for handle in 1..=PROBED_HANDLES {
        let probe = output + ((handle - 1) as usize * PROBE_STRIDE) as i32;
        code.extend([
            asm::load_imm(Reg::A0, handle as i32),
            asm::ecalli(input_status),
            asm::store_u32(Reg::A0, probe),
            asm::load_imm(Reg::A0, handle as i32),
            asm::load_imm(Reg::A1, probe + 12),
            asm::load_imm(Reg::A2, INFO_BYTES as i32),
            asm::ecalli(file_info),
            asm::store_u32(Reg::A0, probe + 4),
            asm::load_imm(Reg::A0, handle as i32),
            asm::load_imm(Reg::A1, probe + 12 + INFO_BYTES as i32),
            asm::load_imm(Reg::A2, DATA_BYTES as i32),
            asm::ecalli(input_read),
            asm::store_u32(Reg::A0, probe + 8),
        ]);
    }
    let asset = output + ASSET_OFFSET as i32;
    code.extend([
        asm::load_imm(Reg::A0, mount_path.0),
        asm::load_imm(Reg::A1, mount_path.1),
        asm::load_imm(Reg::A2, 0),
        asm::load_imm(Reg::A3, asset + 4),
        asm::load_imm(Reg::A4, DATA_BYTES as i32),
        asm::ecalli(asset_read),
        asm::store_u32(Reg::A0, asset),
        asm::load_imm(Reg::A0, results),
        asm::load_imm(Reg::A1, (SNAPSHOT_HEADER_BYTES + OUTPUT_BYTES) as i32),
        asm::ecalli(save_submit),
        asm::ret(),
    ]);
    builder.add_export_by_basic_block(0, b"init");
    builder.add_export_by_basic_block(DONE + 1, b"update");
    builder.set_code(&code, &[]);
    builder.into_vec().unwrap()
}

enum Record<'a> {
    File(&'a str),
    Camera,
    Trigger(u32),
    Cancel(u32),
}

fn descriptors(records: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        let (tag, length, payload): (u32, u32, &[u8]) = match record {
            Record::File(descriptor) => (1, descriptor.len() as u32, descriptor.as_bytes()),
            Record::Camera => (2, 0, &[]),
            Record::Trigger(index) => (3, *index, &[]),
            Record::Cancel(index) => (4, *index, &[]),
        };
        bytes.extend(tag.to_le_bytes());
        bytes.extend(length.to_le_bytes());
        bytes.extend(payload);
    }
    bytes
}

struct Probe {
    status: u32,
    info: Result<String, i32>,
    read: Result<Vec<u8>, i32>,
}

struct Snapshot {
    probes: Vec<Probe>,
    asset: Vec<u8>,
    cancel: u32,
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn snapshot(bytes: &[u8]) -> Snapshot {
    assert_eq!(bytes.len(), SNAPSHOT_HEADER_BYTES + OUTPUT_BYTES);
    let bytes = &bytes[SNAPSHOT_HEADER_BYTES..];
    let probes = (0..PROBED_HANDLES as usize)
        .map(|index| {
            let probe = &bytes[index * PROBE_STRIDE..(index + 1) * PROBE_STRIDE];
            let info_length = i32_at(probe, 4);
            let read_length = i32_at(probe, 8);
            Probe {
                status: i32_at(probe, 0) as u32,
                info: if info_length > 0 {
                    Ok(String::from_utf8(probe[12..12 + info_length as usize].to_vec()).unwrap())
                } else {
                    Err(info_length)
                },
                read: if read_length > 0 {
                    let data = 12 + INFO_BYTES;
                    Ok(probe[data..data + read_length as usize].to_vec())
                } else {
                    Err(read_length)
                },
            }
        })
        .collect();
    let asset_length = i32_at(bytes, ASSET_OFFSET) as usize;
    Snapshot {
        probes,
        asset: bytes[ASSET_OFFSET + 4..ASSET_OFFSET + 4 + asset_length].to_vec(),
        cancel: i32_at(bytes, CANCEL_OFFSET) as u32,
    }
}

fn support(inline: bool, relaunch: bool) -> FileInputSupport {
    FileInputSupport {
        inline,
        relaunch,
        entrypoint: ENTRYPOINT.into(),
    }
}

fn launch(
    records: &[Record<'_>],
    extra_assets: &[(&str, &[u8])],
    support: Option<FileInputSupport>,
    relaunch: Option<FileRelaunch>,
) -> (ApplicationRuntime, Vec<i32>) {
    let mut assets = HashMap::from([(DESCRIPTORS_ASSET.to_owned(), descriptors(records))]);
    for (path, bytes) in extra_assets {
        assets.insert((*path).to_owned(), bytes.to_vec());
    }
    let mut runtime = ApplicationRuntime::new_with_backend(
        FIXTURE,
        assets,
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .unwrap();
    runtime
        .set_mediated_input_kinds(&[CAMERA_KIND.to_owned()])
        .unwrap();
    if let Some(support) = support {
        runtime.set_file_input_support(support).unwrap();
    }
    if let Some(relaunch) = relaunch {
        runtime.set_file_relaunch(relaunch).unwrap();
    }
    runtime.init().unwrap();
    let results = runtime.take_save().unwrap();
    let results = (0..RESULTS)
        .map(|index| i32_at(&results, index * 4))
        .collect();
    (runtime, results)
}

fn update(runtime: &mut ApplicationRuntime) -> Snapshot {
    runtime.update().unwrap();
    snapshot(&runtime.take_save().unwrap())
}

fn selection(name: &str, mime_type: &str, bytes: &[u8]) -> FileSelection {
    FileSelection {
        name: name.into(),
        mime_type: mime_type.into(),
        bytes: bytes.to_vec(),
    }
}

#[test]
fn the_fixture_is_assembled_from_source() {
    assert_eq!(file_input_program(), FIXTURE);
}

#[test]
fn shared_descriptor_vectors_register_through_the_guest() {
    #[derive(serde::Deserialize)]
    struct Vector {
        descriptor: String,
        valid: bool,
    }
    let vectors: Vec<Vector> =
        serde_json::from_str(include_str!("fixtures/file-descriptors.json")).unwrap();
    for vector in vectors {
        let (_, results) = launch(
            &[Record::File(&vector.descriptor)],
            &[],
            Some(support(true, true)),
            None,
        );
        assert_eq!(results[0] > 0, vector.valid, "{}", vector.descriptor);
        if !vector.valid {
            assert_eq!(results[0], MEDIATED_INPUT_REGISTER_INVALID);
        }
    }
}

#[test]
fn registration_shares_handles_and_quota_with_mediated_input() {
    let other = |id: &str| {
        format!(
            r#"{{"id":"{id}","label":"L","extensions":[".{id}"],"delivery":"inline","maxBytes":1}}"#
        )
    };
    let (a, b, c, d, e, f) = (
        other("a"),
        other("b"),
        other("c"),
        other("d"),
        other("e"),
        other("f"),
    );
    let reordered = r#" { "maxBytes" : 16, "delivery" : "inline", "mimeTypes" : ["text/plain"], "extensions" : [".txt"], "label" : "Text document", "id" : "doc" } "#;
    let conflicting = INLINE.replace("\"maxBytes\":16", "\"maxBytes\":17");
    let (runtime, results) = launch(
        &[
            Record::File(INLINE),
            Record::Camera,
            Record::File(reordered),
            Record::File(&conflicting),
            Record::File(RELAUNCH),
            Record::File(&RELAUNCH.replace("\"id\":\"rom\"", "\"id\":\"rom-2\"")),
            Record::File(
                &RELAUNCH
                    .replace("\"id\":\"rom\"", "\"id\":\"rom-3\"")
                    .replace(MOUNT_PATH, ENTRYPOINT),
            ),
            Record::File(&a),
            Record::File(&b),
            Record::File(&c),
            Record::File(&d),
            Record::File(&e),
            Record::File(&f),
            Record::Camera,
        ],
        &[],
        Some(support(true, true)),
        None,
    );
    assert_eq!(
        results[..14],
        [
            1,
            2,
            1,
            MEDIATED_INPUT_REGISTER_INVALID,
            3,
            MEDIATED_INPUT_REGISTER_INVALID,
            MEDIATED_INPUT_REGISTER_INVALID,
            4,
            5,
            6,
            7,
            8,
            MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED,
            2,
        ]
    );
    let registrations = runtime.file_registrations();
    assert_eq!(
        registrations
            .iter()
            .map(|registration| registration.handle)
            .collect::<Vec<_>>(),
        [1, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(registrations[0].descriptor.mime_types, ["text/plain"]);
    assert_eq!(registrations[1].descriptor.delivery, FileDelivery::Relaunch);
    assert_eq!(
        registrations[1].descriptor.mount_path.as_deref(),
        Some(MOUNT_PATH)
    );
}

#[test]
fn unsupported_file_input_and_deliveries_are_reported() {
    let (_, results) = launch(
        &[Record::File(INLINE), Record::File(RELAUNCH)],
        &[],
        None,
        None,
    );
    assert_eq!(
        results[..2],
        [
            MEDIATED_INPUT_REGISTER_UNAVAILABLE,
            MEDIATED_INPUT_REGISTER_UNAVAILABLE
        ]
    );
    let (_, results) = launch(
        &[Record::File(INLINE), Record::File(RELAUNCH)],
        &[],
        Some(support(true, false)),
        None,
    );
    assert_eq!(results[..2], [1, FILE_REGISTER_DELIVERY_UNAVAILABLE]);
    let (_, results) = launch(
        &[Record::File(INLINE), Record::File(RELAUNCH)],
        &[],
        Some(support(false, true)),
        None,
    );
    assert_eq!(results[..2], [FILE_REGISTER_DELIVERY_UNAVAILABLE, 1]);
}

#[test]
fn a_triggered_inline_request_delivers_bytes_and_info() {
    let (mut runtime, results) = launch(
        &[Record::File(INLINE), Record::Trigger(0)],
        &[],
        Some(support(true, true)),
        None,
    );
    assert_eq!(results[..2], [1, 0]);
    let Some(MediatedInputCommand::FileRequest(request)) = runtime.take_mediated_input_command()
    else {
        panic!("expected a file request");
    };
    assert_eq!(request.handle, 1);
    assert_eq!(request.descriptor.id, "doc");
    assert_eq!(update(&mut runtime).probes[0].status, 2);

    let delivery = runtime
        .send_file_input(1, selection("notes/Draft\u{1}.txt", "text/plain", b"hello"))
        .unwrap();
    assert_eq!(delivery, FileInputDelivery::Ready);
    let probe = &update(&mut runtime).probes[0];
    assert_eq!(probe.status, 3);
    assert_eq!(
        probe.info.as_deref(),
        Ok("{\"name\":\"Draft\u{fffd}.txt\",\"mimeType\":\"text/plain\",\"size\":5}")
    );
    assert_eq!(probe.read.as_deref(), Ok(b"hello".as_slice()));

    let probe = &update(&mut runtime).probes[0];
    assert_eq!(probe.status, 1);
    assert_eq!(probe.info, Err(0));
    assert_eq!(probe.read, Err(0));
    assert_eq!(update(&mut runtime).probes[1].info, Err(-1));
}

#[test]
fn host_ui_activates_only_idle_registrations_and_rejects_over_bound_files() {
    let (mut runtime, _) = launch(
        &[Record::File(INLINE), Record::Camera, Record::Trigger(1)],
        &[],
        Some(support(true, true)),
        None,
    );
    assert!(matches!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::Request(_))
    ));
    assert_eq!(
        runtime
            .send_file_input(1, selection("a.txt", "", b"a"))
            .unwrap(),
        FileInputDelivery::Refused
    );
    assert!(runtime
        .send_file_input(2, selection("a.txt", "", b"a"))
        .is_err());
    runtime
        .send_mediated_input_result(2, MediatedInputStatus::Cancelled, Vec::new())
        .unwrap();

    assert_eq!(
        runtime
            .send_file_input(1, selection("big.txt", "", &[0; 17]))
            .unwrap(),
        FileInputDelivery::Rejected
    );
    let probe = &update(&mut runtime).probes[0];
    assert_eq!(probe.status, 6);
    assert_eq!(probe.info, Err(0));
    assert_eq!(probe.read, Err(0));

    assert_eq!(
        runtime
            .send_file_input(1, selection("dropped.txt", "", b"drop"))
            .unwrap(),
        FileInputDelivery::Ready
    );
    assert_eq!(
        runtime
            .send_file_input(1, selection("again.txt", "", b"again"))
            .unwrap(),
        FileInputDelivery::Refused
    );
    assert_eq!(
        update(&mut runtime).probes[0].read.as_deref(),
        Ok(b"drop".as_slice())
    );
    assert!(runtime
        .send_file_input(1, selection("a.txt", "Text/Plain", b"a"))
        .is_err());
}

#[test]
fn a_relaunch_selection_mounts_the_file_in_a_fresh_execution() {
    let records = [Record::File(INLINE), Record::File(RELAUNCH)];
    let packaged = b"packaged cartridge".as_slice();
    let (mut first, results) = launch(
        &records,
        &[(MOUNT_PATH, packaged)],
        Some(support(true, true)),
        None,
    );
    assert_eq!(results[..2], [1, 2]);
    assert_eq!(update(&mut first).asset, packaged);
    assert_eq!(
        first
            .send_file_input(2, selection("roms/big.bin", "", &[0; 33]))
            .unwrap(),
        FileInputDelivery::Rejected
    );
    let FileInputDelivery::Relaunch(relaunch) = first
        .send_file_input(
            2,
            selection(
                "roms/Game.bin",
                "application/octet-stream",
                b"user cartridge",
            ),
        )
        .unwrap()
    else {
        panic!("expected a relaunch");
    };
    assert!(first.is_stopped());
    assert_eq!(relaunch.id, "rom");
    assert_eq!(relaunch.mount_path, MOUNT_PATH);
    assert_eq!(relaunch.name, "Game.bin");
    assert_eq!(first.file_registrations().len(), 2);

    let (mut second, results) = launch(
        &records,
        &[(MOUNT_PATH, packaged)],
        Some(support(true, true)),
        Some(relaunch.clone()),
    );
    assert_eq!(results[..2], [1, 2]);
    assert_eq!(
        second.file_registrations()[1].status,
        MediatedInputStatus::Ready
    );
    let snapshot = update(&mut second);
    assert_eq!(snapshot.asset, b"user cartridge");
    assert_eq!(snapshot.probes[1].status, 3);
    assert_eq!(
        snapshot.probes[1].info.as_deref(),
        Ok(r#"{"name":"Game.bin","mimeType":"application/octet-stream","size":14}"#)
    );
    assert_eq!(snapshot.probes[1].read, Err(0));
    let snapshot = update(&mut second);
    assert_eq!(snapshot.probes[1].status, 1);
    assert_eq!(snapshot.probes[1].info, Err(0));
    assert_eq!(snapshot.asset, b"user cartridge");

    // A handler that returns with another mount path does not claim the file.
    let moved = RELAUNCH.replace(MOUNT_PATH, "game/other.bin");
    let (mut third, _) = launch(
        &[Record::File(&moved)],
        &[],
        Some(support(true, true)),
        Some(relaunch.clone()),
    );
    assert_eq!(update(&mut third).probes[0].status, 1);

    let mut stale = ApplicationRuntime::new_with_backend(
        FIXTURE,
        HashMap::new(),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .unwrap();
    stale.init().unwrap();
    assert!(stale.set_file_relaunch(relaunch).is_err());
}

#[test]
fn relaunch_files_count_toward_asset_bounds() {
    let mut runtime = ApplicationRuntime::new_with_backend(
        FIXTURE,
        HashMap::from([
            ("a.bin".to_owned(), vec![0; 128 * 1024 * 1024]),
            ("b.bin".to_owned(), vec![0; 128 * 1024 * 1024 - 1]),
        ]),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .unwrap();
    let relaunch = |mount_path: &str, length: usize| FileRelaunch {
        id: "rom".into(),
        mount_path: mount_path.into(),
        name: "Game.bin".into(),
        mime_type: String::new(),
        bytes: vec![1; length],
    };
    assert!(runtime.set_file_relaunch(relaunch(MOUNT_PATH, 2)).is_err());
    assert!(runtime
        .set_file_relaunch(relaunch("a.bin", 128 * 1024 * 1024 + 1))
        .is_err());
    assert!(runtime.set_file_relaunch(relaunch(MOUNT_PATH, 1)).is_ok());
    assert!(runtime
        .set_file_relaunch(FileRelaunch {
            name: "dir/Game.bin".into(),
            ..relaunch("a.bin", 1)
        })
        .is_err());
}

#[test]
fn registrations_survive_a_failed_execution() {
    let (mut runtime, _) = launch(
        &[Record::File(INLINE), Record::File(RELAUNCH)],
        &[],
        Some(support(true, true)),
        None,
    );
    let registrations = runtime.take_file_registrations().unwrap();
    assert_eq!(registrations.len(), 2);
    assert_eq!(runtime.take_file_registrations(), None);
    runtime.stop();
    assert_eq!(runtime.file_registrations(), registrations);
    assert!(runtime
        .send_file_input(1, selection("a.txt", "", b"a"))
        .is_err());

    // A trigger naming an index far outside the results traps during `init`.
    let mut failing = ApplicationRuntime::new_with_backend(
        FIXTURE,
        HashMap::from([(
            DESCRIPTORS_ASSET.to_owned(),
            descriptors(&[
                Record::File(INLINE),
                Record::File(RELAUNCH),
                Record::Trigger(1 << 20),
            ]),
        )]),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .unwrap();
    failing.set_file_input_support(support(true, true)).unwrap();
    assert!(failing.init().is_err());
    assert!(failing.is_stopped());
    assert_eq!(failing.file_registrations(), registrations);
}

#[test]
fn cancelling_a_ready_file_returns_the_registration_to_idle() {
    let (mut runtime, results) = launch(
        &[Record::File(INLINE), Record::Cancel(0)],
        &[],
        Some(support(true, true)),
        None,
    );
    assert_eq!(results[..2], [1, 1]);
    let snapshot = update(&mut runtime);
    assert_eq!(snapshot.cancel, MEDIATED_INPUT_CANCEL_NOT_ACTIVE);
    assert_eq!(
        runtime
            .send_file_input(1, selection("a.txt", "", b"abc"))
            .unwrap(),
        FileInputDelivery::Ready
    );
    let snapshot = update(&mut runtime);
    assert_eq!(snapshot.cancel, MEDIATED_INPUT_CANCEL_ACCEPTED);
    assert_eq!(snapshot.probes[0].status, 1);
    assert_eq!(snapshot.probes[0].info, Err(0));
    assert_eq!(snapshot.probes[0].read, Err(0));
    assert_eq!(runtime.take_mediated_input_command(), None);
    assert_eq!(
        update(&mut runtime).cancel,
        MEDIATED_INPUT_CANCEL_NOT_ACTIVE
    );

    let (mut runtime, _) = launch(
        &[Record::File(INLINE), Record::Trigger(0), Record::Cancel(0)],
        &[],
        Some(support(true, true)),
        None,
    );
    assert!(matches!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::FileRequest(_))
    ));
    let snapshot = update(&mut runtime);
    assert_eq!(snapshot.cancel, MEDIATED_INPUT_CANCEL_ACCEPTED);
    assert_eq!(snapshot.probes[0].status, 1);
    assert_eq!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::Cancel { handle: 1 })
    );

    let relaunch = FileRelaunch {
        id: "rom".into(),
        mount_path: MOUNT_PATH.into(),
        name: "Game.bin".into(),
        mime_type: String::new(),
        bytes: b"user cartridge".to_vec(),
    };
    let (mut runtime, _) = launch(
        &[Record::File(RELAUNCH), Record::Cancel(0)],
        &[],
        Some(support(true, true)),
        Some(relaunch),
    );
    let snapshot = update(&mut runtime);
    assert_eq!(snapshot.cancel, MEDIATED_INPUT_CANCEL_ACCEPTED);
    assert_eq!(snapshot.probes[0].status, 1);
    assert_eq!(snapshot.probes[0].info, Err(0));
    assert_eq!(snapshot.asset, b"user cartridge");
    assert_eq!(
        runtime.file_registrations()[0].status,
        MediatedInputStatus::Registered
    );
}
