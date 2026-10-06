/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;
use polkavm_common::program::{asm, InstructionSetKind};
use polkavm_common::writer::ProgramBlobBuilder;

#[test]
fn corevm_accepts_the_motion_hostcall() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_import(b"host_motion_read");
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ecalli(0), asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    assert_eq!(vm.import_motion_read, Some(0));
    assert!(vm.uses_motion());
}

#[test]
fn corevm_reports_when_motion_is_not_imported() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    assert!(!vm.uses_motion());
}

#[test]
fn corevm_accepts_and_reports_the_pointer_capture_hostcall() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_import(crate::POINTER_CAPTURE_IMPORT.as_bytes());
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let mut vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    assert!(vm.uses_pointer_capture());

    vm.set_pointer_capture_supported(true);
    assert_eq!(
        vm.pointer_capture.request(crate::POINTER_CAPTURE_ARM),
        crate::POINTER_CAPTURE_ARMED
    );
    assert_eq!(vm.take_pointer_capture_request(), Some(true));
}

#[test]
fn corevm_capture_state_reaches_the_extended_input_queue() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_import(crate::POINTER_CAPTURE_IMPORT.as_bytes());
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let mut vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    vm.set_pointer_capture_supported(true);
    vm.set_pointer_capture_active(true).unwrap();
    assert_eq!(
        vm.epoca_input_events.pop_front(),
        Some(crate::ui::pointer_capture_record(true))
    );
}

#[test]
fn open_files_enforce_the_descriptor_limit() {
    let file = Arc::new(File { blob: Vec::new() });
    let mut open_files = OpenFiles::new();
    for expected in 3..3 + MAX_OPEN_FILES as u64 {
        assert_eq!(open_files.open(Arc::clone(&file)), Ok(expected));
    }
    assert_eq!(open_files.open(Arc::clone(&file)), Err(EMFILE));
    assert_eq!(open_files.descriptors.len(), MAX_OPEN_FILES);

    assert!(open_files.remove(3).is_some());
    assert_eq!(
        open_files.open(file),
        Ok(3 + MAX_OPEN_FILES as u64),
        "closing a descriptor should restore capacity"
    );
}

#[test]
fn seeks_reject_negative_offsets_without_clamping_valid_offsets() {
    assert_eq!(seek_position(5, 10, -6, SEEK_CUR), Err(EINVAL));
    assert_eq!(seek_position(0, 10, -11, SEEK_END), Err(EINVAL));
    assert_eq!(seek_position(0, 10, 12, SEEK_SET), Ok(12));
    assert_eq!(seek_position(5, 10, 3, SEEK_CUR), Ok(8));
    assert_eq!(seek_position(0, 10, -3, SEEK_END), Ok(7));
}

#[test]
fn wrapped_input_chunks_preserve_order_and_destination() {
    let mut events = VecDeque::with_capacity(4);
    for key in 1..=4 {
        events.push_back(InputEvent { key, value: 1 });
    }
    events.pop_front();
    events.pop_front();
    events.push_back(InputEvent { key: 5, value: 1 });
    events.push_back(InputEvent { key: 6, value: 1 });

    let (first, second) = queued_input_chunks(&events, events.len());
    assert!(
        !second.is_empty(),
        "test queue should cross its ring boundary"
    );
    let keys: Vec<_> = first.iter().chain(second).map(|event| event.key).collect();
    assert_eq!(keys, [3, 4, 5, 6]);
    assert_eq!(
        input_destination(100, first.len()).unwrap(),
        100 + u32::try_from(core::mem::size_of_val(first)).unwrap()
    );

    let written = first.len() + second.len();
    for _ in 0..written {
        events.pop_front();
    }
    assert!(events.is_empty(), "every reported event should be consumed");
}

#[test]
fn legacy_mouse_backlog_keeps_only_the_latest_delta() {
    let mut events = VecDeque::new();
    queue_input_event(&mut events, crate::quake_keys::MOUSE_X, 100);
    queue_input_event(&mut events, crate::quake_keys::MOUSE_X, 80);

    assert_eq!(events.len(), 1);
    assert_eq!(events.front().unwrap().value, 80);
}

#[test]
fn epoca_mouse_backlog_keeps_only_the_latest_frame_delta() {
    let mut events = VecDeque::new();
    let key = crate::InputEvent {
        event_type: crate::InputEventType::KeyDown,
        code: 4,
        x: 0,
        y: 0,
    };
    let stale = crate::InputEvent {
        event_type: crate::InputEventType::PointerDelta,
        code: 0,
        x: 100,
        y: (-60_i16) as u16,
    };
    let latest = crate::InputEvent {
        event_type: crate::InputEventType::PointerDelta,
        code: 0,
        x: 12,
        y: (-7_i16) as u16,
    };

    queue_epoca_input_event(&mut events, key);
    queue_epoca_input_event(&mut events, stale);
    queue_epoca_input_event(&mut events, latest);

    assert_eq!(events.len(), 2);
    assert_eq!(events.pop_front(), Some(key.encode()));
    assert_eq!(events.pop_front(), Some(latest.encode()));
}

mod vector_io {
    use super::*;
    use polkavm_common::abi::MemoryMapBuilder;

    const MEMORY_BYTES: u32 = 64 * 1024;
    const IOV_OFFSET: u32 = 64;
    const OUTPUT_OFFSET: u32 = 512;

    fn memory_address() -> u32 {
        MemoryMapBuilder::new(MEMORY_BYTES)
            .rw_data_size(MEMORY_BYTES)
            .stack_size(4 * 1024)
            .build()
            .unwrap()
            .rw_data_address()
    }

    // Every result comes from an executing guest's syscall followed by pvm_yield.
    // File opening, seeking, and reading all go through that same guest ABI.
    fn guest(vectors: &[(u32, u64, u64)], calls: &[[u64; 4]], file: &[u8]) -> Vm {
        let mut data = vec![0xcc; MEMORY_BYTES as usize];
        data[..10].copy_from_slice(b"input.bin\0");
        data[OUTPUT_OFFSET as usize..OUTPUT_OFFSET as usize + 2].copy_from_slice(b"ok");
        for &(offset, address, length) in vectors {
            let offset = offset as usize;
            data[offset..offset + 8].copy_from_slice(&address.to_le_bytes());
            if offset + 16 <= data.len() {
                data[offset + 8..offset + 16].copy_from_slice(&length.to_le_bytes());
            }
        }
        let mut code = Vec::new();
        let open = [SYS_openat, AT_FDCWD, memory_address() as u64, 0];
        for args in core::iter::once(&open).chain(calls) {
            for (&register, &value) in [Reg::A0, Reg::A1, Reg::A2, Reg::A3].iter().zip(args) {
                code.push(asm::load_imm(register, value as i32));
            }
            code.push(asm::ecalli(0));
            code.push(asm::ecalli(1));
        }
        code.push(asm::ret());
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest64);
        builder.set_rw_data_size(MEMORY_BYTES);
        builder.set_rw_data(data);
        builder.set_stack_size(4 * 1024);
        builder.add_import(b"pvm_syscall");
        builder.add_import(b"pvm_yield");
        builder.add_export_by_basic_block(0, b"_pvm_start");
        builder.set_code(&code, &[]);
        let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
        let mut vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
        vm.register_file("input.bin", file.to_vec());
        vm.setup(ComputerContext::default()).unwrap();
        vm.set_gas(1_000_000);
        assert_eq!(result(&mut vm), 3);
        vm
    }

    fn result(vm: &mut Vm) -> i32 {
        assert!(matches!(vm.run().unwrap(), Interruption::Yield));
        vm.instance.reg(Reg::A0) as i32
    }

    fn output(vm: &mut Vm, offset: u32, length: u32) -> Vec<u8> {
        vm.instance
            .read_memory(memory_address() + offset, length)
            .unwrap()
    }

    #[test]
    fn readv_preserves_progress_and_file_position_after_later_faults() {
        let base = memory_address() as u64;
        // Missing descriptor, missing length field, unmapped buffer, and a
        // non-canonical buffer must all preserve the completed first vector.
        for (offset, bad_buffer) in [
            (MEMORY_BYTES - 16, 0),
            (MEMORY_BYTES - 24, 0),
            (IOV_OFFSET, 0),
            (IOV_OFFSET, (1_u64 << 32) + base + OUTPUT_OFFSET as u64),
        ] {
            let mut vectors = vec![(offset, base + OUTPUT_OFFSET as u64, 2)];
            if offset + 16 < MEMORY_BYTES {
                vectors.push((offset + 16, bad_buffer, 2));
            }
            let mut vm = guest(
                &vectors,
                &[
                    [SYS_readv, 3, base + offset as u64, 2],
                    [SYS_lseek, 3, 0, SEEK_CUR],
                    [SYS_read, 3, base + OUTPUT_OFFSET as u64 + 16, 5],
                ],
                &[1, 2, 3, 4, 5],
            );
            assert_eq!(result(&mut vm), 2);
            assert_eq!(output(&mut vm, OUTPUT_OFFSET, 3), [1, 2, 0xcc]);
            assert_eq!(result(&mut vm), 2);
            assert_eq!(result(&mut vm), 3);
            assert_eq!(output(&mut vm, OUTPUT_OFFSET + 16, 4), [3, 4, 5, 0xcc]);
        }
    }

    #[test]
    fn readv_returns_efault_without_consuming_file_when_nothing_was_read() {
        let base = memory_address() as u64;
        for offset in [MEMORY_BYTES, MEMORY_BYTES - 8, IOV_OFFSET] {
            let vectors = if offset == MEMORY_BYTES {
                vec![]
            } else {
                vec![(offset, 0, 1)]
            };
            let mut vm = guest(
                &vectors,
                &[
                    [SYS_readv, 3, base + offset as u64, 1],
                    [SYS_lseek, 3, 0, SEEK_CUR],
                    [SYS_read, 3, base + OUTPUT_OFFSET as u64, 3],
                ],
                &[1, 2, 3],
            );
            assert_eq!(result(&mut vm), -(EFAULT as i32));
            assert_eq!(result(&mut vm), 0);
            assert_eq!(result(&mut vm), 3);
            assert_eq!(output(&mut vm, OUTPUT_OFFSET, 3), [1, 2, 3]);
        }
    }

    #[test]
    fn readv_zero_length_does_not_hide_the_next_error() {
        let base = memory_address() as u64;
        let mut vm = guest(
            &[
                (IOV_OFFSET, base + OUTPUT_OFFSET as u64, 0),
                (IOV_OFFSET + 16, 0, 1),
            ],
            &[
                [SYS_readv, 3, base + IOV_OFFSET as u64, 2],
                [SYS_read, 3, base + OUTPUT_OFFSET as u64, 3],
            ],
            &[1, 2, 3],
        );
        assert_eq!(result(&mut vm), -(EFAULT as i32));
        assert_eq!(result(&mut vm), 3);
        assert_eq!(output(&mut vm, OUTPUT_OFFSET, 3), [1, 2, 3]);
    }

    #[test]
    fn readv_stops_at_short_reads_and_returns_zero_at_eof() {
        let base = memory_address() as u64;
        let mut vm = guest(
            &[
                (IOV_OFFSET, base + OUTPUT_OFFSET as u64, 2),
                (IOV_OFFSET + 16, base + OUTPUT_OFFSET as u64 + 4, 6),
                (IOV_OFFSET + 32, 0, 1),
            ],
            &[
                [SYS_readv, 3, base + IOV_OFFSET as u64, 3],
                [SYS_readv, 3, base + IOV_OFFSET as u64, 3],
                [SYS_lseek, 3, 0, SEEK_CUR],
            ],
            &[1, 2, 3, 4, 5],
        );
        assert_eq!(result(&mut vm), 5);
        assert_eq!(
            output(&mut vm, OUTPUT_OFFSET, 8),
            [1, 2, 0xcc, 0xcc, 3, 4, 5, 0xcc]
        );
        assert_eq!(result(&mut vm), 0);
        assert_eq!(result(&mut vm), 5);
    }

    #[test]
    fn writev_preserves_progress_after_later_descriptor_and_buffer_faults() {
        let base = memory_address() as u64;
        for (offset, bad_buffer) in [
            (MEMORY_BYTES - 16, 0),
            (MEMORY_BYTES - 24, 0),
            (IOV_OFFSET, 0),
            (IOV_OFFSET, (1_u64 << 32) + base + OUTPUT_OFFSET as u64),
        ] {
            let mut vectors = vec![(offset, base + OUTPUT_OFFSET as u64, 2)];
            if offset + 16 < MEMORY_BYTES {
                vectors.push((offset + 16, bad_buffer, 2));
            }
            let mut vm = guest(
                &vectors,
                &[
                    [SYS_writev, FILENO_STDOUT, base + offset as u64, 2],
                    [SYS_writev, FILENO_STDERR, base + offset as u64, 1],
                ],
                &[],
            );
            assert_eq!(result(&mut vm), 2);
            assert_eq!(result(&mut vm), 2);
        }
    }

    #[test]
    fn writev_returns_efault_without_progress_and_ebadf_for_invalid_fd() {
        let base = memory_address() as u64;
        let mut vm = guest(
            &[
                (IOV_OFFSET, base + OUTPUT_OFFSET as u64, 0),
                (IOV_OFFSET + 16, 0, 1),
            ],
            &[
                [SYS_writev, FILENO_STDOUT, base + MEMORY_BYTES as u64, 1],
                [SYS_writev, FILENO_STDERR, base + MEMORY_BYTES as u64 - 8, 1],
                [SYS_writev, FILENO_STDOUT, base + IOV_OFFSET as u64, 2],
                [SYS_writev, 3, base + IOV_OFFSET as u64, 1],
            ],
            &[],
        );
        assert_eq!(result(&mut vm), -(EFAULT as i32));
        assert_eq!(result(&mut vm), -(EFAULT as i32));
        assert_eq!(result(&mut vm), -(EFAULT as i32));
        assert_eq!(result(&mut vm), -(EBADF as i32));
    }

    #[test]
    fn writev_stops_after_the_host_transfer_limit_short_write() {
        let base = memory_address() as u64;
        let mut vm = guest(
            &[
                (
                    IOV_OFFSET,
                    base + OUTPUT_OFFSET as u64,
                    MAX_GUEST_WRITE_BYTES + 1,
                ),
                (IOV_OFFSET + 16, base + OUTPUT_OFFSET as u64, 1),
            ],
            &[[SYS_writev, FILENO_STDOUT, base + IOV_OFFSET as u64, 2]],
            &[],
        );
        assert_eq!(result(&mut vm), MAX_GUEST_WRITE_BYTES as i32);
    }
}
