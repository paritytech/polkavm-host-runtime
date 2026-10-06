/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! File-input conformance through `tests/fixtures/file-input.polkavm`.
//!
//! The fixture is assembled by [`file_input_program`], so any Host platform
//! reproduces it byte for byte. During `init` the guest walks the
//! `descriptors` asset, a sequence of `[tag u32][length u32][payload]` records
//! ended by tag 0: tag 1 registers the payload with `host_file_register` and
//! tag 2 registers `camera-ur` with `host_input_register`. Tags 3, 4 and 5 carry
//! no payload; their `length` indexes an earlier record, whose handle tag 3
//! triggers, tag 4 selects for cancellation on update, and tag 5 cancels
//! immediately. The guest saves the sixteen `i32` results. Every `update`
//! cancels the selected handle with
//! `host_input_cancel`, probes handles 1 through 4 with `host_input_status`,
//! `host_file_info`, and `host_input_read`, reads the `game/cartridge.bin`
//! asset, and saves the results followed by one [`Snapshot`].

use polkavm::Reg;
use polkavm_common::abi::MemoryMapBuilder;
use polkavm_common::program::{asm, Instruction, InstructionSetKind};
use polkavm_common::writer::ProgramBlobBuilder;
use polkavm_host_runtime::{
    ApplicationRuntime, BackendKind, FileCache, FileDelivery, FileInputDelivery, FileInputSupport,
    FileReadSource, FileRelaunch, FileSelection, FileStreamSelection, LocalFileCache,
    LocalFileSource, MediatedInputCommand, MediatedInputStatus, PresentationProfile, Runtime,
    FILE_READ_INVALID_DESTINATION, FILE_READ_INVALID_HANDLE, FILE_READ_INVALID_RANGE,
    FILE_READ_IO_ERROR, FILE_REGISTER_DELIVERY_UNAVAILABLE, MAX_FILE_CACHE_BYTES,
    MAX_FILE_READ_BYTES, MEDIATED_INPUT_CANCEL_ACCEPTED, MEDIATED_INPUT_CANCEL_NOT_ACTIVE,
    MEDIATED_INPUT_REGISTER_INVALID, MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED,
    MEDIATED_INPUT_REGISTER_UNAVAILABLE, MEDIATED_INPUT_TRIGGER_ACCEPTED,
    MEDIATED_INPUT_TRIGGER_BUSY,
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

    // Basic blocks: 0 init, 1 loop, 2 tag 2?, 3 tag 3?, 4 tag 4?, 5 tag 5?,
    // 6 file, 7 input, 8 trigger, 9 cancel, 10 cancel now, 11 store, 12 done,
    // 13 update.
    const LOOP: u32 = 1;
    const INPUT: u32 = 7;
    const TRIGGER: u32 = 8;
    const CANCEL: u32 = 9;
    const CANCEL_NOW: u32 = 10;
    const STORE: u32 = 11;
    const DONE: u32 = 12;
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
        asm::branch_eq_imm(Reg::T0, 5, CANCEL_NOW),
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
        asm::jump(STORE),
        // CANCEL NOW
        asm::shift_logical_left_imm_32(Reg::T2, Reg::T1, 2),
        asm::add_imm_32(Reg::T2, Reg::T2, results),
        asm::load_indirect_i32(Reg::A0, Reg::T2, 0),
        asm::ecalli(input_cancel),
        asm::load_imm(Reg::T1, 0),
        asm::jump(STORE),
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
        stream: false,
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
#[ignore = "regenerates the shared Rust/browser guest fixture"]
fn export_file_input_fixture() {
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/file-input.polkavm"
        ),
        file_input_program(),
    )
    .unwrap();
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
            Some(FileInputSupport {
                stream: true,
                ..support(true, true)
            }),
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
    let Some(MediatedInputCommand::Request(request)) = runtime.take_mediated_input_command() else {
        panic!("expected camera request");
    };
    assert!(
        request.handle >= 0x8000_0000,
        "camera tokens cannot alias file handles"
    );
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
        .send_mediated_input_result(request.handle, MediatedInputStatus::Cancelled, Vec::new())
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

const STREAM_DESCRIPTOR: &str = r#"{"id":"stream","label":"Stream file","extensions":[".bin"],"delivery":"stream","maxBytes":4294967295}"#;
const STREAM_COMMANDS: &str = "stream-commands";
const STREAM_INFO_OFFSET: usize = 20;
const STREAM_DATA_OFFSET: usize = STREAM_INFO_OFFSET + 256;
const STREAM_SNAPSHOT_BYTES: usize = STREAM_DATA_OFFSET + MAX_FILE_READ_BYTES as usize;

/// Each update consumes [opcode, handle, offset, length, capacity].
/// 0 probe, 1 source read, 2 cancel, 3 picker, 4 invalid source destination,
/// 5 cache reset (size=offset), 6 cache write, 7 cache commit, 8 cache read,
/// 9 invalid cache write pointer, 10 invalid cache read pointer,
/// 11 register stream2..stream8 (index=offset, 0..6).
/// Writes use the deterministic byte pattern `(offset + index) & 255`.
/// Snapshots retain the original stream fixture layout.
fn file_stream_program() -> Vec<u8> {
    let extra_descriptors: Vec<String> = (2..=8)
        .map(|index| {
            STREAM_DESCRIPTOR.replace("\"id\":\"stream\"", &format!("\"id\":\"stream{index}\""))
        })
        .collect();
    let extra_descriptor_size = extra_descriptors[0].len() as i32;
    let mut ro_data = [STREAM_DESCRIPTOR, STREAM_COMMANDS].concat().into_bytes();
    let extra_offset = ro_data.len() as i32;
    for descriptor in extra_descriptors {
        ro_data.extend_from_slice(descriptor.as_bytes());
    }
    let pattern_offset = ro_data.len() as i32;
    ro_data.extend((0..MAX_FILE_READ_BYTES + 255).map(|index| index as u8));
    let memory = MemoryMapBuilder::new(64 * 1024)
        .ro_data_size(ro_data.len() as u32)
        .rw_data_size(128 * 1024)
        .stack_size(4 * 1024)
        .build()
        .unwrap();
    let descriptor = memory.ro_data_address() as i32;
    let asset = descriptor + STREAM_DESCRIPTOR.len() as i32;
    let extra = descriptor + extra_offset;
    let pattern = descriptor + pattern_offset;
    let command = memory.rw_data_address() as i32;
    let cursor = command + 20;
    let output = cursor + 4;
    let info = output + STREAM_INFO_OFFSET as i32;
    let data = output + STREAM_DATA_OFFSET as i32;
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_ro_data_size(ro_data.len() as u32);
    builder.set_ro_data(ro_data);
    builder.set_rw_data_size(128 * 1024);
    builder.set_stack_size(4 * 1024);
    for import in [
        "host_file_register",
        "host_asset_read",
        "host_file_read",
        "host_input_cancel",
        "host_input_trigger",
        "host_input_status",
        "host_file_info",
        "host_input_read",
        "host_save_submit",
        "host_update_after",
        "host_file_cache_reset",
        "host_file_cache_write",
        "host_file_cache_commit",
        "host_file_cache_read",
    ] {
        builder.add_import(import.as_bytes());
    }
    let [register, asset_read, file_read, cancel, trigger, status, file_info, input_read, save, schedule, cache_reset, cache_write, cache_commit, cache_read] =
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
    const OPERATION_BLOCK: u32 = 14;
    const SNAPSHOT_BLOCK: u32 = OPERATION_BLOCK + 12;
    let mut code = vec![
        // Block 0: init.
        asm::load_imm(Reg::A0, descriptor),
        asm::load_imm(Reg::A1, STREAM_DESCRIPTOR.len() as i32),
        asm::ecalli(register),
        asm::store_u32(Reg::A0, output),
        asm::load_imm(Reg::A0, output),
        asm::load_imm(Reg::A1, 4),
        asm::ecalli(save),
        asm::load_imm(Reg::A0, -1),
        asm::ecalli(schedule),
        asm::ret(),
        // Block 1: update and first dispatch branch.
        asm::load_imm(Reg::A0, asset),
        asm::load_imm(Reg::A1, STREAM_COMMANDS.len() as i32),
        asm::load_i32(Reg::A2, cursor),
        asm::load_imm(Reg::A3, command),
        asm::load_imm(Reg::A4, 20),
        asm::ecalli(asset_read),
        asm::load_i32(Reg::T0, cursor),
        asm::add_imm_32(Reg::T0, Reg::T0, 20),
        asm::store_u32(Reg::T0, cursor),
        asm::load_i32(Reg::S0, command),
        asm::load_i32(Reg::S1, command + 4),
    ];
    for opcode in 0..12 {
        code.push(asm::branch_eq_imm(
            Reg::S0,
            opcode,
            OPERATION_BLOCK + opcode as u32,
        ));
    }
    // Block 13: unknown operation, metadata-only probe.
    code.extend([asm::load_imm(Reg::A0, 0), asm::jump(SNAPSHOT_BLOCK)]);
    for opcode in 0..12 {
        code.push(asm::move_reg(Reg::A0, Reg::S1));
        match opcode {
            0 => code.push(asm::load_imm(Reg::A0, 0)),
            1 | 4 | 6 | 8 | 9 | 10 => {
                code.extend([
                    asm::load_i32(Reg::A1, command + 8),
                    asm::load_i32(Reg::A3, command + 12),
                ]);
                if matches!(opcode, 4 | 9 | 10) {
                    code.push(asm::load_imm(Reg::A2, -1));
                } else if opcode == 6 {
                    code.extend([
                        asm::and_imm(Reg::A2, Reg::A1, 255),
                        asm::add_imm_32(Reg::A2, Reg::A2, pattern),
                    ]);
                } else {
                    code.push(asm::load_imm(Reg::A2, data));
                }
                code.push(asm::ecalli(match opcode {
                    1 | 4 => file_read,
                    6 | 9 => cache_write,
                    _ => cache_read,
                }));
            }
            2 => code.push(asm::ecalli(cancel)),
            3 => code.push(asm::ecalli(trigger)),
            5 => code.extend([
                asm::load_i32(Reg::A1, command + 8),
                asm::ecalli(cache_reset),
            ]),
            7 => code.push(asm::ecalli(cache_commit)),
            11 => code.extend([
                asm::load_i32(Reg::A0, command + 8),
                asm::mul_imm_32(Reg::A0, Reg::A0, extra_descriptor_size),
                asm::add_imm_32(Reg::A0, Reg::A0, extra),
                asm::load_imm(Reg::A1, extra_descriptor_size),
                asm::ecalli(register),
            ]),
            _ => unreachable!(),
        }
        code.push(asm::jump(SNAPSHOT_BLOCK));
    }
    code.extend([
        asm::store_u32(Reg::A0, output),
        asm::move_reg(Reg::A0, Reg::S1),
        asm::ecalli(status),
        asm::store_u32(Reg::A0, output + 4),
        asm::move_reg(Reg::A0, Reg::S1),
        asm::load_imm(Reg::A1, info),
        asm::load_imm(Reg::A2, 256),
        asm::ecalli(file_info),
        asm::store_u32(Reg::A0, output + 8),
        asm::move_reg(Reg::A0, Reg::S1),
        asm::load_imm(Reg::A1, data),
        asm::load_i32(Reg::A2, command + 16),
        asm::ecalli(input_read),
        asm::store_u32(Reg::A0, output + 12),
        asm::move_reg(Reg::A0, Reg::S1),
        asm::ecalli(status),
        asm::store_u32(Reg::A0, output + 16),
        asm::load_imm(Reg::A0, output),
        asm::load_imm(Reg::A1, STREAM_SNAPSHOT_BYTES as i32),
        asm::ecalli(save),
        asm::load_imm(Reg::A0, -1),
        asm::ecalli(schedule),
        asm::ret(),
    ]);
    builder.add_export_by_basic_block(0, b"init");
    builder.add_export_by_basic_block(1, b"update");
    builder.set_code(&code, &[]);
    builder.into_vec().unwrap()
}

#[test]
#[ignore = "regenerates the shared Rust/browser guest fixture"]
fn export_file_stream_fixture() {
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/file-stream.polkavm"
        ),
        file_stream_program(),
    )
    .unwrap();
}

fn launch_stream(commands: &[[u32; 5]]) -> ApplicationRuntime {
    let bytes = commands
        .iter()
        .flatten()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let mut runtime = ApplicationRuntime::new_with_backend(
        &file_stream_program(),
        HashMap::from([(STREAM_COMMANDS.to_owned(), bytes)]),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .unwrap();
    runtime
        .set_file_input_support(FileInputSupport {
            stream: true,
            ..support(false, false)
        })
        .unwrap();
    runtime.init().unwrap();
    assert_eq!(runtime.take_save().unwrap(), 1u32.to_le_bytes());
    runtime
}

fn stream_update(runtime: &mut ApplicationRuntime) -> Vec<u8> {
    runtime.update().unwrap();
    let snapshot = runtime.take_save().unwrap();
    assert_eq!(snapshot.len(), STREAM_SNAPSHOT_BYTES);
    assert_eq!(
        i32_at(&snapshot, 12),
        0,
        "stream host_input_read must not consume metadata"
    );
    assert_eq!(i32_at(&snapshot, 4), i32_at(&snapshot, 16));
    snapshot
}

fn stream_info(snapshot: &[u8]) -> Option<serde_json::Value> {
    let length = i32_at(snapshot, 8);
    (length > 0).then(|| {
        serde_json::from_slice(&snapshot[STREAM_INFO_OFFSET..STREAM_INFO_OFFSET + length as usize])
            .unwrap()
    })
}

struct DiskFixture(std::path::PathBuf);

impl DiskFixture {
    fn new(bytes: &[u8]) -> Self {
        use std::io::Write;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("polkavm-stream-{}-{id}", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(bytes).unwrap();
        Self(path)
    }

    fn source(&self) -> LocalFileSource {
        LocalFileSource::new(std::fs::File::open(&self.0).unwrap()).unwrap()
    }
}

impl Drop for DiskFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct ObservedFile {
    source: LocalFileSource,
    reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl FileReadSource for ObservedFile {
    fn size(&self) -> u64 {
        self.source.size()
    }

    fn read_exact_at(&mut self, offset: u32, destination: &mut [u8]) -> anyhow::Result<()> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.source.read_exact_at(offset, destination)
    }
}

impl Drop for ObservedFile {
    fn drop(&mut self) {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn observed_file(
    file: &DiskFixture,
) -> (
    Box<dyn FileReadSource>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    (
        Box::new(ObservedFile {
            source: file.source(),
            reads: reads.clone(),
            closed: closed.clone(),
        }),
        reads,
        closed,
    )
}

fn stream_selection(size: u64) -> FileStreamSelection {
    FileStreamSelection {
        name: "host/path/selected.bin".into(),
        mime_type: String::new(),
        size,
    }
}

#[test]
fn stream_reads_are_positional_bounded_and_do_not_consume_the_selection() {
    use std::sync::atomic::Ordering::SeqCst;
    let contents: Vec<u8> = (0..MAX_FILE_READ_BYTES + 19)
        .map(|offset| (offset % 251) as u8)
        .collect();
    let file = DiskFixture::new(&contents);
    let (source, reads, closed) = observed_file(&file);
    let mut runtime = launch_stream(&[
        [0, 1, 0, 0, 0],
        [1, 1, 7, MAX_FILE_READ_BYTES, 0],
        [1, 1, MAX_FILE_READ_BYTES + 15, 64, 0],
        [1, 1, contents.len() as u32, 1, 0],
        [1, 1, 0, 4, 0],
    ]);
    assert_eq!(
        runtime
            .send_file_stream(1, stream_selection(contents.len() as u64), source, None)
            .unwrap(),
        FileInputDelivery::Ready
    );
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 4), 3);
    assert_eq!(stream_info(&snapshot).unwrap()["name"], "selected.bin");
    assert_eq!(reads.load(SeqCst), 0);
    for (offset, length) in [
        (7, MAX_FILE_READ_BYTES as usize),
        (MAX_FILE_READ_BYTES as usize + 15, 4),
        (contents.len(), 0),
        (0, 4),
    ] {
        let snapshot = stream_update(&mut runtime);
        assert_eq!(i32_at(&snapshot, 0), length as i32);
        assert_eq!(i32_at(&snapshot, 4), 3);
        assert_eq!(stream_info(&snapshot).unwrap()["size"], contents.len());
        assert_eq!(
            &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + length],
            &contents[offset..offset + length]
        );
    }
    assert_eq!(reads.load(SeqCst), 3, "EOF does not perform disk I/O");
    assert!(!closed.load(SeqCst));
    runtime.stop();
    assert!(
        closed.load(SeqCst),
        "stop releases sources before command drain"
    );
}

#[test]
fn invalid_stream_reads_preserve_state_and_never_touch_the_source() {
    use std::sync::atomic::Ordering::SeqCst;
    let file = DiskFixture::new(b"abcdef");
    let (source, reads, closed) = observed_file(&file);
    let mut runtime = launch_stream(&[
        [1, 1, 0, 1, 0],
        [1, 99, 0, 1, 0],
        [1, 1, 0, 0, 0],
        [1, 1, 0, MAX_FILE_READ_BYTES + 1, 0],
        [1, 1, 7, 1, 0],
        [4, 1, 0, 4, 0],
        [1, 1, 2, 3, 0],
    ]);
    assert_eq!(
        i32_at(&stream_update(&mut runtime), 0),
        FILE_READ_INVALID_HANDLE
    );
    runtime
        .send_file_stream(1, stream_selection(6), source, None)
        .unwrap();
    for expected in [
        FILE_READ_INVALID_HANDLE,
        FILE_READ_INVALID_RANGE,
        FILE_READ_INVALID_RANGE,
        FILE_READ_INVALID_RANGE,
        FILE_READ_INVALID_DESTINATION,
    ] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
        assert_eq!(
            runtime.file_registrations()[0].status,
            MediatedInputStatus::Ready
        );
    }
    assert_eq!(reads.load(SeqCst), 0);
    assert!(!closed.load(SeqCst));
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 0), 3);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 3],
        b"cde"
    );
    assert_eq!(reads.load(SeqCst), 1);
}

#[test]
fn cancel_reselect_and_refused_candidates_release_only_the_correct_source() {
    use std::sync::atomic::Ordering::SeqCst;
    let file = DiskFixture::new(b"first");
    let second = DiskFixture::new(b"second");
    let mut runtime = launch_stream(&[
        [1, 1, 0, 5, 0],
        [3, 1, 0, 0, 0],
        [1, 1, 0, 6, 0],
        [2, 1, 0, 0, 0],
        [1, 1, 0, 1, 0],
    ]);
    let (source, _, closed) = observed_file(&file);
    runtime
        .send_file_stream(1, stream_selection(5), source, None)
        .unwrap();
    let (candidate, _, refused_closed) = observed_file(&second);
    assert_eq!(
        runtime
            .send_file_stream(1, stream_selection(6), candidate, None)
            .unwrap(),
        FileInputDelivery::Refused
    );
    assert!(refused_closed.load(SeqCst));
    assert!(!closed.load(SeqCst));
    assert!(runtime
        .send_mediated_input_result(1, MediatedInputStatus::Failed, vec![])
        .is_err());
    let snapshot = stream_update(&mut runtime);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 5],
        b"first"
    );
    assert_eq!(i32_at(&stream_update(&mut runtime), 4), 2);
    assert!(
        closed.load(SeqCst),
        "picker replacement closes before command drain"
    );
    assert_eq!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::Cancel { handle: 1 })
    );
    assert!(matches!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::FileRequest(_))
    ));
    let (source, _, second_closed) = observed_file(&second);
    runtime
        .send_file_stream(1, stream_selection(6), source, None)
        .unwrap();
    let snapshot = stream_update(&mut runtime);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 6],
        b"second"
    );
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 0), 0);
    assert_eq!(i32_at(&snapshot, 4), 1);
    assert!(stream_info(&snapshot).is_none());
    assert!(second_closed.load(SeqCst));
    assert_eq!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::Cancel { handle: 1 })
    );
    assert_eq!(
        i32_at(&stream_update(&mut runtime), 0),
        FILE_READ_INVALID_HANDLE
    );
}

#[test]
fn short_disk_read_fails_atomically_and_releases_the_selection() {
    use std::sync::atomic::Ordering::SeqCst;
    let file = DiskFixture::new(b"abcdef");
    let (source, reads, closed) = observed_file(&file);
    let mut runtime = launch_stream(&[[1, 1, 0, 6, 0], [1, 1, 0, 1, 0]]);
    runtime
        .send_file_stream(1, stream_selection(6), source, None)
        .unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file.0)
        .unwrap()
        .set_len(2)
        .unwrap();
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 0), FILE_READ_IO_ERROR);
    assert_eq!(i32_at(&snapshot, 4), 6);
    assert!(stream_info(&snapshot).is_none());
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 6],
        &[0; 6],
        "partial reads never reach guest memory"
    );
    assert_eq!(reads.load(SeqCst), 1);
    assert!(closed.load(SeqCst));
    assert_eq!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::Cancel { handle: 1 })
    );
    assert_eq!(
        i32_at(&stream_update(&mut runtime), 0),
        FILE_READ_INVALID_HANDLE
    );
}

#[test]
fn selection_size_and_delivery_must_match_the_retained_source() {
    use std::sync::atomic::Ordering::SeqCst;
    let file = DiskFixture::new(b"abc");
    let mut runtime = launch_stream(&[]);
    let (source, _, closed) = observed_file(&file);
    assert!(runtime
        .send_file_stream(1, stream_selection(4), source, None)
        .is_err());
    assert!(closed.load(SeqCst));
    assert_eq!(
        runtime.file_registrations()[0].status,
        MediatedInputStatus::Registered
    );
    assert!(runtime
        .send_file_input(1, selection("file.bin", "", b"abc"))
        .is_err());
    let empty = DiskFixture::new(b"");
    let (source, _, closed) = observed_file(&empty);
    assert_eq!(
        runtime
            .send_file_stream(1, stream_selection(0), source, None)
            .unwrap(),
        FileInputDelivery::Rejected
    );
    assert!(closed.load(SeqCst));
    assert_eq!(
        runtime.file_registrations()[0].status,
        MediatedInputStatus::Failed
    );
    let (mut inline, _) = launch(
        &[Record::File(INLINE)],
        &[],
        Some(support(true, false)),
        None,
    );
    assert!(inline
        .send_file_stream(1, stream_selection(3), Box::new(file.source()), None)
        .is_err());
    let small =
        r#"{"id":"small","label":"Small","extensions":[".bin"],"delivery":"stream","maxBytes":2}"#;
    let (mut bounded, _) = launch(
        &[Record::File(small)],
        &[],
        Some(FileInputSupport {
            stream: true,
            ..support(false, false)
        }),
        None,
    );
    let (source, _, closed) = observed_file(&file);
    assert_eq!(
        bounded
            .send_file_stream(1, stream_selection(3), source, None)
            .unwrap(),
        FileInputDelivery::Rejected
    );
    assert!(closed.load(SeqCst));
    assert_eq!(
        bounded.file_registrations()[0].status,
        MediatedInputStatus::Failed
    );
    let (source, _, closed) = observed_file(&file);
    runtime
        .send_file_stream(1, stream_selection(3), source, None)
        .unwrap();
    drop(runtime);
    assert!(
        closed.load(SeqCst),
        "dropping the execution releases the selected source"
    );
}

#[test]
fn sparse_files_above_the_relaunch_limit_are_read_without_asset_mounts() {
    let file = DiskFixture::new(b"");
    let size = 129 * 1024 * 1024u64;
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&file.0)
        .unwrap();
    writer.set_len(size).unwrap();
    use std::io::{Seek, SeekFrom, Write};
    writer.seek(SeekFrom::Start(size - 4)).unwrap();
    writer.write_all(b"last").unwrap();
    drop(writer);
    let mut runtime = launch_stream(&[[1, 1, size as u32 - 4, 64, 0]]);
    runtime
        .send_file_stream(1, stream_selection(size), Box::new(file.source()), None)
        .unwrap();
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 0), 4);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 4],
        b"last"
    );
    assert_eq!(stream_info(&snapshot).unwrap()["size"], size);
}

#[test]
#[ignore = "reads a Host-selected local file, never a checked-in asset"]
fn stream_selected_disk_smoke() {
    use std::io::{Read, Seek, SeekFrom};
    let path = std::env::var_os("POLKAVM_STREAM_SMOKE_FILE")
        .expect("set POLKAVM_STREAM_SMOKE_FILE to the selected disk file");
    let file = std::fs::File::open(&path).unwrap();
    let source = LocalFileSource::new(file).unwrap();
    let size = source.size();
    assert!(size >= 2048 && size <= u32::MAX as u64);
    let mut expected = std::fs::File::open(&path).unwrap();
    let mut header = [0; 2048];
    expected.read_exact(&mut header).unwrap();
    let mut ending = [0; 32];
    expected.seek(SeekFrom::End(-32)).unwrap();
    expected.read_exact(&mut ending).unwrap();
    let mut runtime = launch_stream(&[
        [1, 1, 0, 2048, 0],
        [1, 1, size as u32 - 32, 64, 0],
        [1, 1, size as u32, 1, 0],
        [1, 1, 0, 2048, 0],
        [2, 1, 0, 0, 0],
    ]);
    assert_eq!(
        runtime
            .send_file_stream(1, stream_selection(size), Box::new(source), None)
            .unwrap(),
        FileInputDelivery::Ready
    );
    for bytes in [header.as_slice(), ending.as_slice(), &[], header.as_slice()] {
        let snapshot = stream_update(&mut runtime);
        assert_eq!(i32_at(&snapshot, 0), bytes.len() as i32);
        assert_eq!(i32_at(&snapshot, 4), 3);
        assert_eq!(
            &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + bytes.len()],
            bytes
        );
    }
    assert_eq!(i32_at(&stream_update(&mut runtime), 4), 1);
    println!("selected disk stream: {size} bytes; positional reads 2048/32/0/2048; cancel returned idle; header {:02x?}", &header[..16]);
}

struct CacheDirectory(std::path::PathBuf);

impl CacheDirectory {
    fn new() -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let path = std::env::temp_dir().join(format!(
            "polkavm-cache-test-{:032x}",
            u128::from_le_bytes(random)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn cache(&self) -> LocalFileCache {
        LocalFileCache::new(&self.0).unwrap()
    }

    fn assert_empty(&self) {
        assert_eq!(std::fs::read_dir(&self.0).unwrap().count(), 0);
    }
}

impl Drop for CacheDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum CacheFault {
    Reset = 1,
    Write = 2,
    Flush = 3,
    Read = 4,
    Size = 5,
}

#[derive(Default)]
struct CacheObservation {
    reads: std::sync::atomic::AtomicUsize,
    writes: std::sync::atomic::AtomicUsize,
    resets: std::sync::atomic::AtomicUsize,
    flushes: std::sync::atomic::AtomicUsize,
    closed: std::sync::atomic::AtomicBool,
    fault: std::sync::atomic::AtomicU8,
}

impl CacheObservation {
    fn failing(&self, fault: CacheFault) -> bool {
        self.fault.load(std::sync::atomic::Ordering::SeqCst) == fault as u8
    }
}

/// Fault injection wraps real disk I/O; it never supplies synthetic cache bytes.
struct ObservedCache {
    cache: LocalFileCache,
    observation: std::sync::Arc<CacheObservation>,
}

impl FileReadSource for ObservedCache {
    fn size(&self) -> u64 {
        self.cache.size() + u64::from(self.observation.failing(CacheFault::Size))
    }

    fn read_exact_at(&mut self, offset: u32, destination: &mut [u8]) -> anyhow::Result<()> {
        self.observation
            .reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.observation.failing(CacheFault::Read) {
            self.cache.read_exact_at(offset, &mut destination[..1])?;
            anyhow::bail!("injected short disk cache read");
        }
        self.cache.read_exact_at(offset, destination)
    }
}

impl FileCache for ObservedCache {
    fn reset(&mut self, size: u32) -> anyhow::Result<()> {
        self.observation
            .resets
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.cache.reset(size)?;
        if self.observation.failing(CacheFault::Reset) {
            anyhow::bail!("injected reset failure after truncation");
        }
        Ok(())
    }

    fn write_exact_at(&mut self, offset: u32, bytes: &[u8]) -> anyhow::Result<()> {
        self.observation
            .writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.observation.failing(CacheFault::Write) {
            self.cache.write_exact_at(offset, &bytes[..1])?;
            anyhow::bail!("injected short disk cache write");
        }
        self.cache.write_exact_at(offset, bytes)
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        self.observation
            .flushes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.cache.flush()?;
        if self.observation.failing(CacheFault::Flush) {
            anyhow::bail!("injected disk cache flush failure");
        }
        Ok(())
    }
}

impl Drop for ObservedCache {
    fn drop(&mut self) {
        self.observation
            .closed
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn observed_cache(
    directory: &CacheDirectory,
) -> (Box<dyn FileCache>, std::sync::Arc<CacheObservation>) {
    let observation = std::sync::Arc::new(CacheObservation::default());
    (
        Box::new(ObservedCache {
            cache: directory.cache(),
            observation: observation.clone(),
        }),
        observation,
    )
}

fn cache_selection(
    runtime: &mut ApplicationRuntime,
    handle: u32,
    source: &DiskFixture,
    cache: Box<dyn FileCache>,
) {
    let source = source.source();
    assert_eq!(
        runtime
            .send_file_stream(
                handle,
                stream_selection(source.size()),
                Box::new(source),
                Some(cache)
            )
            .unwrap(),
        FileInputDelivery::Ready
    );
}

#[test]
fn private_disk_cache_seals_complete_sequential_chunks_and_preserves_the_source() {
    use std::sync::atomic::Ordering::SeqCst;
    let directory = CacheDirectory::new();
    let source = DiskFixture::new(b"original compressed source");
    let (cache, observation) = observed_cache(&directory);
    let size = MAX_FILE_READ_BYTES + 23;
    let mut runtime = launch_stream(&[
        [5, 1, size, 0, 0],
        [8, 1, 0, 1, 0],
        [7, 1, 0, 0, 0],
        [6, 1, 0, 7, 0],
        [6, 1, 7, MAX_FILE_READ_BYTES, 0],
        [7, 1, 0, 0, 0],
        [6, 1, MAX_FILE_READ_BYTES + 7, 16, 0],
        [7, 1, 0, 0, 0],
        [8, 1, 0, MAX_FILE_READ_BYTES, 0],
        [8, 1, MAX_FILE_READ_BYTES - 3, 64, 0],
        [8, 1, size, 1, 0],
        [6, 1, size, 1, 0],
        [7, 1, 0, 0, 0],
        [1, 1, 0, 64, 0],
        [5, 1, 4, 0, 0],
        [8, 1, 0, 4, 0],
        [6, 1, 0, 4, 0],
        [7, 1, 0, 0, 0],
        [8, 1, 0, 8, 0],
    ]);
    cache_selection(&mut runtime, 1, &source, cache);
    for expected in [0, -1, -2, 7, MAX_FILE_READ_BYTES as i32, -2, 16, 0] {
        let snapshot = stream_update(&mut runtime);
        assert_eq!(i32_at(&snapshot, 0), expected);
        assert_eq!(i32_at(&snapshot, 4), 3);
        assert_eq!(stream_info(&snapshot).unwrap()["size"], 26);
    }
    for (offset, length) in [(0, MAX_FILE_READ_BYTES), (MAX_FILE_READ_BYTES - 3, 26)] {
        let snapshot = stream_update(&mut runtime);
        assert_eq!(i32_at(&snapshot, 0), length as i32);
        let expected: Vec<u8> = (offset..offset + length).map(|index| index as u8).collect();
        assert_eq!(
            &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + length as usize],
            expected
        );
    }
    for expected in [0, -2, -2] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    assert_eq!(observation.reads.load(SeqCst), 2, "EOF does not read disk");
    let snapshot = stream_update(&mut runtime);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 26],
        b"original compressed source"
    );
    for expected in [0, -1, 4, 0] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 0), 4);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 4],
        &[0, 1, 2, 3]
    );
    runtime.stop();
    assert!(observation.closed.load(SeqCst));
    directory.assert_empty();
}

#[test]
fn invalid_cache_requests_preserve_cursor_seal_and_disk_contents_without_io() {
    use std::sync::atomic::Ordering::SeqCst;
    let directory = CacheDirectory::new();
    let source = DiskFixture::new(b"source");
    let (cache, observation) = observed_cache(&directory);
    let mut runtime = launch_stream(&[
        [5, 1, 4, 0, 0],
        [5, 1, 0, 0, 0],
        [5, 1, MAX_FILE_CACHE_BYTES + 1, 0, 0],
        [6, 1, 1, 1, 0],
        [6, 1, 0, 0, 0],
        [6, 1, 0, MAX_FILE_READ_BYTES + 1, 0],
        [6, 1, 0, 5, 0],
        [9, 1, 0, 4, 0],
        [6, 1, 0, 4, 0],
        [7, 1, 0, 0, 0],
        [10, 1, 0, 4, 0],
        [8, 1, 0, 0, 0],
        [8, 1, 0, MAX_FILE_READ_BYTES + 1, 0],
        [8, 1, 5, 1, 0],
        [5, 1, 0, 0, 0],
        [8, 1, 0, 4, 0],
    ]);
    cache_selection(&mut runtime, 1, &source, cache);
    for expected in [0, -2, -2, -2, -2, -2, -2, -3] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    assert_eq!(observation.resets.load(SeqCst), 1);
    assert_eq!(observation.writes.load(SeqCst), 0);
    for expected in [4, 0, -3, -2, -2, -2, -2] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    assert_eq!(observation.reads.load(SeqCst), 0);
    assert_eq!(observation.writes.load(SeqCst), 1);
    assert_eq!(observation.flushes.load(SeqCst), 1);
    let snapshot = stream_update(&mut runtime);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 4],
        &[0, 1, 2, 3]
    );
    assert_eq!(observation.resets.load(SeqCst), 1);
}

#[test]
fn cache_reservations_are_aggregate_and_released_on_cancel_and_reset() {
    use std::sync::atomic::Ordering::SeqCst;
    let directory = CacheDirectory::new();
    let source = DiskFixture::new(b"source");
    let (first, first_observation) = observed_cache(&directory);
    let (second, second_observation) = observed_cache(&directory);
    let mut runtime = launch_stream(&[
        [11, 1, 0, 0, 0],
        [5, 1, MAX_FILE_CACHE_BYTES - 1, 0, 0],
        [5, 2, 2, 0, 0],
        [5, 1, 1, 0, 0],
        [5, 2, MAX_FILE_CACHE_BYTES - 1, 0, 0],
        [5, 2, MAX_FILE_CACHE_BYTES, 0, 0],
        [2, 1, 0, 0, 0],
        [5, 2, MAX_FILE_CACHE_BYTES, 0, 0],
    ]);
    assert_eq!(i32_at(&stream_update(&mut runtime), 0), 2);
    cache_selection(&mut runtime, 1, &source, first);
    cache_selection(&mut runtime, 2, &source, second);
    for expected in [0, -2] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    assert_eq!(
        second_observation.resets.load(SeqCst),
        0,
        "quota is checked before truncation"
    );
    for expected in [0, 0, -2, MEDIATED_INPUT_CANCEL_ACCEPTED as i32, 0] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    assert!(first_observation.closed.load(SeqCst));
    assert_eq!(second_observation.resets.load(SeqCst), 2);
    runtime.stop();
    assert!(second_observation.closed.load(SeqCst));
    directory.assert_empty();
}

#[test]
fn cache_backend_failures_drop_only_the_cache_and_release_its_reservation() {
    use std::sync::atomic::Ordering::SeqCst;
    for fault in [
        CacheFault::Reset,
        CacheFault::Size,
        CacheFault::Write,
        CacheFault::Flush,
        CacheFault::Read,
    ] {
        let directory = CacheDirectory::new();
        let source_file = DiskFixture::new(b"source");
        let (source, _, source_closed) = observed_file(&source_file);
        let (cache, observation) = observed_cache(&directory);
        let mut commands = vec![[11, 1, 0, 0, 0], [5, 1, MAX_FILE_CACHE_BYTES, 0, 0]];
        if matches!(fault, CacheFault::Reset | CacheFault::Size) {
            commands.push([5, 1, MAX_FILE_CACHE_BYTES, 0, 0]);
        } else {
            commands.extend([[5, 1, 4, 0, 0], [6, 1, 0, 4, 0]]);
            if matches!(fault, CacheFault::Flush | CacheFault::Read) {
                commands.push([7, 1, 0, 0, 0]);
            }
            if matches!(fault, CacheFault::Read) {
                commands.push([8, 1, 1, 3, 0]);
            }
        }
        let failure_index = commands.len() - 1;
        commands.extend([
            [5, 1, 1, 0, 0],
            [8, 1, 0, 1, 0],
            [5, 2, MAX_FILE_CACHE_BYTES, 0, 0],
            [1, 1, 0, 6, 0],
        ]);
        let mut runtime = launch_stream(&commands);
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), 2);
        runtime
            .send_file_stream(1, stream_selection(6), source, Some(cache))
            .unwrap();
        cache_selection(&mut runtime, 2, &source_file, Box::new(directory.cache()));
        for command in commands.iter().take(failure_index).skip(1) {
            assert_eq!(
                i32_at(&stream_update(&mut runtime), 0),
                if command[0] == 6 { 4 } else { 0 }
            );
        }
        observation.fault.store(fault as u8, SeqCst);
        let snapshot = stream_update(&mut runtime);
        assert_eq!(i32_at(&snapshot, 0), FILE_READ_IO_ERROR);
        assert_eq!(i32_at(&snapshot, 4), 3);
        if matches!(fault, CacheFault::Read) {
            assert_eq!(
                &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 3],
                &[0; 3],
                "short read bytes never reach guest memory"
            );
        }
        assert!(observation.closed.load(SeqCst));
        assert!(!source_closed.load(SeqCst));
        for expected in [-1, -1, 0] {
            assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
        }
        let snapshot = stream_update(&mut runtime);
        assert_eq!(
            &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 6],
            b"source"
        );
        runtime.stop();
        assert!(source_closed.load(SeqCst));
        directory.assert_empty();
    }
}

#[test]
fn cache_ownership_follows_selection_refusal_reselection_cancellation_and_stop() {
    use std::sync::atomic::Ordering::SeqCst;
    let directory = CacheDirectory::new();
    let file = DiskFixture::new(b"source");
    let mut runtime = launch_stream(&[
        [5, 1, 4, 0, 0],
        [6, 1, 0, 4, 0],
        [7, 1, 0, 0, 0],
        [8, 1, 0, 4, 0],
        [3, 1, 0, 0, 0],
        [5, 1, 1, 0, 0],
        [2, 1, 0, 0, 0],
        [5, 1, 1, 0, 0],
    ]);
    let (source, _, first_source_closed) = observed_file(&file);
    let (cache, first) = observed_cache(&directory);
    runtime
        .send_file_stream(1, stream_selection(6), source, Some(cache))
        .unwrap();
    for expected in [0, 4, 0] {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    let (candidate_source, _, candidate_closed) = observed_file(&file);
    let (candidate_cache, candidate) = observed_cache(&directory);
    assert_eq!(
        runtime
            .send_file_stream(
                1,
                stream_selection(6),
                candidate_source,
                Some(candidate_cache)
            )
            .unwrap(),
        FileInputDelivery::Refused
    );
    assert!(candidate_closed.load(SeqCst));
    assert!(candidate.closed.load(SeqCst));
    assert!(!first.closed.load(SeqCst));
    let snapshot = stream_update(&mut runtime);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 4],
        &[0, 1, 2, 3]
    );
    assert_eq!(i32_at(&stream_update(&mut runtime), 0), 0);
    assert!(first.closed.load(SeqCst));
    assert!(first_source_closed.load(SeqCst));
    assert_eq!(
        i32_at(&stream_update(&mut runtime), 0),
        -1,
        "picker has no Ready cache"
    );
    let (cache, replacement) = observed_cache(&directory);
    cache_selection(&mut runtime, 1, &file, cache);
    assert_eq!(
        i32_at(&stream_update(&mut runtime), 0),
        MEDIATED_INPUT_CANCEL_ACCEPTED as i32
    );
    assert!(replacement.closed.load(SeqCst));
    assert_eq!(i32_at(&stream_update(&mut runtime), 0), -1);
    let (cache, last) = observed_cache(&directory);
    cache_selection(&mut runtime, 1, &file, cache);
    runtime.stop();
    assert!(last.closed.load(SeqCst));
    directory.assert_empty();
}

#[test]
fn absent_and_rejected_caches_never_gain_guest_access() {
    use std::sync::atomic::Ordering::SeqCst;
    let directory = CacheDirectory::new();
    let file = DiskFixture::new(b"source");
    let mut runtime = launch_stream(&[
        [5, 1, 1, 0, 0],
        [6, 1, 0, 1, 0],
        [7, 1, 0, 0, 0],
        [8, 1, 0, 1, 0],
        [1, 1, 0, 6, 0],
    ]);
    let (cache, observation) = observed_cache(&directory);
    assert!(runtime
        .send_file_stream(1, stream_selection(5), Box::new(file.source()), Some(cache))
        .is_err());
    assert!(observation.closed.load(SeqCst));
    let empty = DiskFixture::new(&[]);
    let (cache, observation) = observed_cache(&directory);
    assert_eq!(
        runtime
            .send_file_stream(
                1,
                stream_selection(0),
                Box::new(empty.source()),
                Some(cache)
            )
            .unwrap(),
        FileInputDelivery::Rejected
    );
    assert!(observation.closed.load(SeqCst));
    runtime
        .send_file_stream(1, stream_selection(6), Box::new(file.source()), None)
        .unwrap();
    for _ in 0..4 {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), -1);
    }
    let snapshot = stream_update(&mut runtime);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 6],
        b"source"
    );
    drop(runtime);
    directory.assert_empty();
}

#[test]
fn local_file_cache_truncates_old_contents_and_has_a_private_temporary_lifetime() {
    let directory = CacheDirectory::new();
    let mut cache = directory.cache();
    #[cfg(unix)]
    directory.assert_empty();
    cache.reset(8).unwrap();
    cache.write_exact_at(0, b"private!").unwrap();
    cache.flush().unwrap();
    let mut bytes = [0; 8];
    cache.read_exact_at(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"private!");
    cache.reset(4).unwrap();
    assert_eq!(cache.size(), 4);
    cache.read_exact_at(0, &mut bytes[..4]).unwrap();
    assert_eq!(&bytes[..4], &[0; 4]);
    assert!(cache.read_exact_at(0, &mut bytes).is_err());
    assert!(cache.write_exact_at(3, b"past-end").is_err());
    drop(cache);
    directory.assert_empty();
}

#[test]
fn eight_selected_caches_share_one_reservation_limit() {
    let directory = CacheDirectory::new();
    let source = DiskFixture::new(b"source");
    let mut commands: Vec<[u32; 5]> = (0..7).map(|index| [11, 1, index, 0, 0]).collect();
    commands.extend((1..=8).map(|handle| [5, handle, MAX_FILE_CACHE_BYTES / 8, 0, 0]));
    commands.extend([
        [5, 8, MAX_FILE_CACHE_BYTES / 8 + 1, 0, 0],
        [2, 1, 0, 0, 0],
        [5, 8, MAX_FILE_CACHE_BYTES / 4, 0, 0],
    ]);
    let mut runtime = launch_stream(&commands);
    for expected in 2..=8 {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), expected);
    }
    assert_eq!(runtime.file_registrations().len(), 8);
    for handle in 1..=8 {
        cache_selection(&mut runtime, handle, &source, Box::new(directory.cache()));
    }
    for _ in 0..8 {
        assert_eq!(i32_at(&stream_update(&mut runtime), 0), 0);
    }
    assert_eq!(i32_at(&stream_update(&mut runtime), 0), -2);
    assert_eq!(
        i32_at(&stream_update(&mut runtime), 0),
        MEDIATED_INPUT_CANCEL_ACCEPTED as i32
    );
    assert_eq!(i32_at(&stream_update(&mut runtime), 0), 0);
    runtime.stop();
    directory.assert_empty();
}

#[test]
fn source_io_failure_releases_its_private_cache_and_drop_closes_retained_caches() {
    use std::sync::atomic::Ordering::SeqCst;
    let directory = CacheDirectory::new();
    let file = DiskFixture::new(b"source");
    let (source, _, source_closed) = observed_file(&file);
    let (cache, observation) = observed_cache(&directory);
    let mut runtime = launch_stream(&[[5, 1, 4, 0, 0], [1, 1, 0, 6, 0], [5, 1, 1, 0, 0]]);
    runtime
        .send_file_stream(1, stream_selection(6), source, Some(cache))
        .unwrap();
    assert_eq!(i32_at(&stream_update(&mut runtime), 0), 0);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file.0)
        .unwrap()
        .set_len(2)
        .unwrap();
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 0), FILE_READ_IO_ERROR);
    assert_eq!(i32_at(&snapshot, 4), MediatedInputStatus::Failed as i32);
    assert!(source_closed.load(SeqCst));
    assert!(observation.closed.load(SeqCst));
    assert_eq!(i32_at(&stream_update(&mut runtime), 0), -1);
    let (cache, replacement) = observed_cache(&directory);
    cache_selection(&mut runtime, 1, &file, cache);
    drop(runtime);
    assert!(replacement.closed.load(SeqCst));
    directory.assert_empty();
}

#[test]
fn raw_runtime_traps_release_stream_and_cache_without_waiting_for_drop() {
    use std::sync::atomic::Ordering::SeqCst;

    let memory = MemoryMapBuilder::new(64 * 1024)
        .ro_data_size(STREAM_DESCRIPTOR.len() as u32)
        .stack_size(4 * 1024)
        .build()
        .unwrap();
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_ro_data_size(STREAM_DESCRIPTOR.len() as u32);
    builder.set_ro_data(STREAM_DESCRIPTOR.as_bytes().to_vec());
    builder.set_stack_size(4 * 1024);
    builder.add_import(b"host_file_register");
    builder.add_export_by_basic_block(0, b"init");
    builder.add_export_by_basic_block(1, b"update");
    builder.set_code(
        &[
            asm::load_imm(Reg::A0, memory.ro_data_address() as i32),
            asm::load_imm(Reg::A1, STREAM_DESCRIPTOR.len() as i32),
            asm::ecalli(0),
            asm::ret(),
            asm::trap(),
        ],
        &[],
    );
    let mut runtime = Runtime::new_with_backend(
        &builder.into_vec().unwrap(),
        HashMap::new(),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .unwrap();
    runtime
        .set_file_input_support(FileInputSupport {
            stream: true,
            ..support(false, false)
        })
        .unwrap();
    runtime.init().unwrap();
    let directory = CacheDirectory::new();
    let file = DiskFixture::new(b"source");
    let (source, _, source_closed) = observed_file(&file);
    let (cache, observation) = observed_cache(&directory);
    assert_eq!(
        runtime
            .send_file_stream(1, stream_selection(6), source, Some(cache))
            .unwrap(),
        FileInputDelivery::Ready
    );
    assert!(runtime.update().is_err());
    assert!(source_closed.load(SeqCst));
    assert!(observation.closed.load(SeqCst));
    assert_eq!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::Cancel { handle: 1 })
    );
    assert_eq!(runtime.file_registrations().len(), 1);
    assert_eq!(
        runtime.file_registrations()[0].status,
        MediatedInputStatus::Registered
    );
    let (source, _, rejected_source_closed) = observed_file(&file);
    let (cache, rejected_cache) = observed_cache(&directory);
    assert!(runtime
        .send_file_stream(1, stream_selection(6), source, Some(cache))
        .is_err());
    assert!(rejected_source_closed.load(SeqCst));
    assert!(rejected_cache.closed.load(SeqCst));
    assert!(runtime.init().unwrap_err().to_string().contains("stopped"));
    assert!(runtime
        .update()
        .unwrap_err()
        .to_string()
        .contains("stopped"));
    directory.assert_empty();
}

#[test]
fn paused_stream_delivery_waits_for_resume_without_losing_its_resources() {
    use std::sync::atomic::Ordering::SeqCst;

    let mut runtime = launch_stream(&[[1, 1, 0, 6, 0]]);
    runtime.set_paused(true);
    let directory = CacheDirectory::new();
    let file = DiskFixture::new(b"source");
    let (source, reads, source_closed) = observed_file(&file);
    let (cache, observation) = observed_cache(&directory);
    assert_eq!(
        runtime
            .send_file_stream(1, stream_selection(6), source, Some(cache))
            .unwrap(),
        FileInputDelivery::Ready
    );
    runtime.update().unwrap();
    assert!(runtime.take_save().is_none());
    assert_eq!(reads.load(SeqCst), 0);
    assert!(!source_closed.load(SeqCst));
    assert!(!observation.closed.load(SeqCst));
    runtime.set_paused(false);
    let snapshot = stream_update(&mut runtime);
    assert_eq!(i32_at(&snapshot, 0), 6);
    assert_eq!(
        &snapshot[STREAM_DATA_OFFSET..STREAM_DATA_OFFSET + 6],
        b"source"
    );
    assert_eq!(reads.load(SeqCst), 1);
    runtime.stop();
    assert!(source_closed.load(SeqCst));
    assert!(observation.closed.load(SeqCst));
}

#[test]
fn picker_command_backpressure_preserves_selection_and_recovers_after_host_drain() {
    use std::sync::atomic::Ordering::SeqCst;

    let mut commands = Vec::new();
    for _ in 0..4 {
        commands.extend([[3, 1, 0, 0, 0], [2, 1, 0, 0, 0]]);
    }
    // Repeated rejected triggers must not enqueue or release a selected file.
    commands.extend(std::iter::repeat_n([3, 1, 0, 0, 0], 32));
    commands.push([3, 1, 0, 0, 0]);
    let mut runtime = launch_stream(&commands);
    for _ in 0..4 {
        assert_eq!(
            i32_at(&stream_update(&mut runtime), 0),
            MEDIATED_INPUT_TRIGGER_ACCEPTED as i32
        );
        assert_eq!(
            i32_at(&stream_update(&mut runtime), 0),
            MEDIATED_INPUT_CANCEL_ACCEPTED as i32
        );
    }
    let directory = CacheDirectory::new();
    let file = DiskFixture::new(b"source");
    let (source, _, source_closed) = observed_file(&file);
    let (cache, observation) = observed_cache(&directory);
    runtime
        .send_file_stream(1, stream_selection(6), source, Some(cache))
        .unwrap();
    for _ in 0..32 {
        let snapshot = stream_update(&mut runtime);
        assert_eq!(i32_at(&snapshot, 0), MEDIATED_INPUT_TRIGGER_BUSY as i32);
        assert_eq!(i32_at(&snapshot, 4), MediatedInputStatus::Ready as i32);
        assert!(stream_info(&snapshot).is_some());
        assert!(!source_closed.load(SeqCst));
        assert!(!observation.closed.load(SeqCst));
    }
    for _ in 0..4 {
        assert!(matches!(
            runtime.take_mediated_input_command(),
            Some(MediatedInputCommand::FileRequest(request)) if request.handle == 1
        ));
        assert_eq!(
            runtime.take_mediated_input_command(),
            Some(MediatedInputCommand::Cancel { handle: 1 })
        );
    }
    assert_eq!(runtime.take_mediated_input_command(), None);
    assert_eq!(
        i32_at(&stream_update(&mut runtime), 0),
        MEDIATED_INPUT_TRIGGER_ACCEPTED as i32
    );
    assert!(source_closed.load(SeqCst));
    assert!(observation.closed.load(SeqCst));
    assert_eq!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::Cancel { handle: 1 })
    );
    assert!(matches!(
        runtime.take_mediated_input_command(),
        Some(MediatedInputCommand::FileRequest(request)) if request.handle == 1
    ));
    assert_eq!(runtime.take_mediated_input_command(), None);
}
