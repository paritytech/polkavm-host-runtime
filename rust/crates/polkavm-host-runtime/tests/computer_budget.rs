/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Supervisor fairness under guest request loops, pipes, and workspace children.

use polkavm::Reg;
use polkavm_common::abi::MemoryMapBuilder;
use polkavm_common::program::{asm, Instruction, InstructionSetKind};
use polkavm_common::writer::ProgramBlobBuilder;
use polkavm_host_runtime::{BackendKind, ComputerContext, ComputerStatus, ComputerSupervisor};

const REQUESTS: i32 = 10_000;
const WAIT: i32 = 0;
const RUN: i32 = 1;
const SPAWN: i32 = 2;
const WRITE: i32 = 3;
const EXIT: i32 = 4;
const READ: i32 = 5;
const WORKSPACE_SPAWN: i32 = 6;
const WORKSPACE_READ: i32 = 7;
const WORKSPACE_WAIT: i32 = 8;
const DRIVER: &[u8] = include_bytes!("fixtures/computer-pipe-driver.polkavm");
const FILTER: &[u8] = include_bytes!("fixtures/computer-pipe-filter.polkavm");

fn memory_address() -> i32 {
    MemoryMapBuilder::new(64 * 1024)
        .rw_data_size(64 * 1024)
        .stack_size(4 * 1024)
        .build()
        .unwrap()
        .rw_data_address() as i32
}

fn program(code: &[Instruction]) -> Vec<u8> {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_rw_data_size(64 * 1024);
    builder.set_rw_data(b"missing\0worker".to_vec());
    builder.set_stack_size(4 * 1024);
    for import in [
        b"polkadot_host_0_1_process_wait".as_slice(),
        b"polkadot_host_0_1_process_run".as_slice(),
        b"polkadot_host_0_1_process_spawn".as_slice(),
        b"polkadot_host_0_1_tty_write".as_slice(),
        b"polkadot_host_0_1_core_exit".as_slice(),
        b"polkadot_host_0_1_pipe_read".as_slice(),
        b"polkadot_host_0_1_workspace_spawn".as_slice(),
        b"polkadot_host_0_1_workspace_read".as_slice(),
        b"polkadot_host_0_1_workspace_wait".as_slice(),
    ] {
        builder.add_import(import);
    }
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(code, &[]);
    builder.into_vec().unwrap()
}

fn launch(import: i32, offset: i32, length: i32) -> Vec<Instruction> {
    vec![
        asm::load_imm(Reg::A0, memory_address() + offset),
        asm::load_imm(Reg::A1, length),
        asm::load_imm(Reg::A2, 0),
        asm::load_imm(Reg::A3, 0),
        asm::ecalli(import),
    ]
}

fn report_status() -> Vec<Instruction> {
    let output = memory_address() + 32;
    vec![
        asm::store_u8(Reg::A0, output),
        asm::load_imm(Reg::A0, 1),
        asm::load_imm(Reg::A1, output),
        asm::load_imm(Reg::A2, 1),
        asm::ecalli(WRITE),
    ]
}

fn repeat(code: &mut Vec<Instruction>, count: i32, body: &[Instruction]) {
    code.extend([asm::load_imm(Reg::S0, count), asm::fallthrough()]);
    let target = code
        .iter()
        .filter(|instruction| instruction.opcode().starts_new_basic_block())
        .count() as u32;
    code.extend_from_slice(body);
    code.extend([
        asm::add_imm_32(Reg::S0, Reg::S0, -1),
        asm::branch_not_eq_imm(Reg::S0, 0, target),
    ]);
}

fn supervisor(program: &[u8]) -> ComputerSupervisor {
    ComputerSupervisor::new_with_backend(
        program,
        ComputerContext::default(),
        1_000_000,
        BackendKind::Interpreter,
    )
    .unwrap()
}

fn finish(supervisor: &mut ComputerSupervisor, output: &mut Vec<u8>) -> i32 {
    // All guest loops are finite, including on the vulnerable implementation.
    // A regression fails an assertion rather than depending on a timeout.
    for _ in 0..32 {
        let status = supervisor.run().unwrap();
        while let Some(bytes) = supervisor.take_terminal_output() {
            output.extend(bytes);
        }
        match status {
            ComputerStatus::Exited(code) => return code,
            ComputerStatus::Yielded => {}
            other => panic!("supervisor exposed an unresolved request: {other:?}"),
        }
    }
    panic!("finite guest did not finish within 32 scheduling turns");
}

fn check_hostile_requests(import: i32, expected_status: u8) {
    let mut request = if import == WAIT {
        vec![asm::load_imm(Reg::A0, 999), asm::ecalli(WAIT)]
    } else {
        launch(import, 0, 7)
    };
    request.extend(report_status());
    let mut code = Vec::new();
    repeat(&mut code, REQUESTS, &request);
    // Valid work after the hostile loop exercises foreground spawn, pipes,
    // background spawn/wait and output forwarding across the scheduling yield.
    code.extend(launch(RUN, 8, 6));
    code.extend([asm::ecalli(EXIT), asm::trap()]);
    let mut supervisor = supervisor(&program(&code));
    supervisor
        .register_package("worker", DRIVER.to_vec())
        .unwrap();
    supervisor
        .register_package("upper", FILTER.to_vec())
        .unwrap();

    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Yielded);
    let mut output = supervisor.take_terminal_output().unwrap_or_default();
    assert!(output.len() < REQUESTS as usize);
    assert_eq!(finish(&mut supervisor, &mut output), 0);
    let mut expected = vec![expected_status; REQUESTS as usize];
    expected.extend_from_slice(b"HELLO, PIPES");
    assert_eq!(output, expected);
}

#[test]
fn invalid_wait_loop_yields_and_valid_work_resumes() {
    check_hostile_requests(WAIT, (-2i8) as u8);
}

#[test]
fn unknown_foreground_package_loop_yields_and_valid_work_resumes() {
    check_hostile_requests(RUN, (-4i8) as u8);
}

#[test]
fn unknown_background_package_loop_yields_and_valid_work_resumes() {
    check_hostile_requests(SPAWN, (-4i8) as u8);
}

#[test]
fn background_requests_share_the_host_turn_budget_and_preserve_pipe_output() {
    let mut request = vec![asm::load_imm(Reg::A0, 999), asm::ecalli(WAIT)];
    request.extend(report_status());
    let mut child = Vec::new();
    repeat(&mut child, REQUESTS, &request);
    child.extend([asm::load_imm(Reg::A0, 23), asm::ecalli(EXIT), asm::trap()]);

    let mut parent = launch(SPAWN, 8, 6);
    parent.push(asm::move_reg(Reg::S1, Reg::A0));
    let output = memory_address() + 32;
    let read = [
        asm::move_reg(Reg::A0, Reg::S1),
        asm::load_imm(Reg::A1, output),
        asm::load_imm(Reg::A2, 16_384),
        asm::ecalli(READ),
        // Forward received bytes; EOF writes zero bytes and WOULD_BLOCK's
        // negative length is rejected by tty_write without emitting anything.
        asm::move_reg(Reg::A2, Reg::A0),
        asm::load_imm(Reg::A0, 1),
        asm::load_imm(Reg::A1, output),
        asm::ecalli(WRITE),
    ];
    repeat(&mut parent, 32, &read);
    parent.extend([
        asm::move_reg(Reg::A0, Reg::S1),
        asm::ecalli(WAIT),
        asm::ecalli(EXIT),
        asm::trap(),
    ]);
    let mut supervisor = supervisor(&program(&parent));
    supervisor
        .register_package("worker", program(&child))
        .unwrap();

    // The foreground makes only 34 requests: bounding its loop alone is not
    // enough. The child's 10,000 resumptions must consume the same budget.
    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Yielded);
    let mut output = supervisor.take_terminal_output().unwrap_or_default();
    assert!(output.len() < REQUESTS as usize);
    assert_eq!(finish(&mut supervisor, &mut output), 23);
    assert_eq!(output, vec![(-5i8) as u8; REQUESTS as usize]);
}

#[test]
fn workspace_children_share_the_budget_and_preserve_terminal_output() {
    let mut request = vec![asm::load_imm(Reg::A0, 999), asm::ecalli(WAIT)];
    request.extend(report_status());
    let mut child = Vec::new();
    repeat(&mut child, REQUESTS, &request);
    child.extend([asm::load_imm(Reg::A0, 23), asm::ecalli(EXIT), asm::trap()]);

    let mut parent = vec![asm::load_imm(Reg::A4, 80), asm::load_imm(Reg::A5, 24)];
    parent.extend(launch(WORKSPACE_SPAWN, 8, 6));
    parent.push(asm::move_reg(Reg::S1, Reg::A0));
    let output = memory_address() + 32;
    let read = [
        asm::move_reg(Reg::A0, Reg::S1),
        asm::load_imm(Reg::A1, output),
        asm::load_imm(Reg::A2, 16_384),
        asm::ecalli(WORKSPACE_READ),
        asm::move_reg(Reg::A2, Reg::A0),
        asm::load_imm(Reg::A0, 1),
        asm::load_imm(Reg::A1, output),
        asm::ecalli(WRITE),
    ];
    repeat(&mut parent, 32, &read);
    parent.extend([
        asm::move_reg(Reg::A0, Reg::S1),
        asm::ecalli(WORKSPACE_WAIT),
        asm::ecalli(EXIT),
        asm::trap(),
    ]);
    let mut supervisor = supervisor(&program(&parent));
    supervisor.set_workspace_enabled(true);
    supervisor
        .register_package("worker", program(&child))
        .unwrap();

    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Yielded);
    let mut output = supervisor.take_terminal_output().unwrap_or_default();
    assert!(output.len() < REQUESTS as usize);
    assert_eq!(finish(&mut supervisor, &mut output), 23);
    assert_eq!(output, vec![(-2i8) as u8; REQUESTS as usize]);
}
