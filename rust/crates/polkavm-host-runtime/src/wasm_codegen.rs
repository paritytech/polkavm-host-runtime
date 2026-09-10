/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{
    MAX_GUEST_HEAP_BYTES, MAX_GUEST_RW_DATA_BYTES, MAX_GUEST_STACK_BYTES, MAX_PROGRAM_BYTES,
};
use anyhow::{anyhow, bail, Context, Result};
use polkavm::program::{Instruction as PvmInstruction, ParsedInstruction, RawReg};
use polkavm::{MemoryMapBuilder, ProgramBlob, Reg, RETURN_TO_HOST};
use std::borrow::Cow;
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, CustomSection, DataSection, ElementSection, Elements,
    EntityType, ExportKind, ExportSection, Function, FunctionSection, GlobalSection, GlobalType,
    ImportSection, Instruction as W, MemArg, MemorySection, MemoryType, Module, RefType,
    TableSection, TableType, TypeSection, ValType,
};

const PAGE_SIZE: u32 = 65_536;
const STATUS_FINISHED: i32 = -1;
const STATUS_ECALL: i32 = -2;
const STATUS_TRAP: i32 = -3;
const STATUS_OUT_OF_GAS: i32 = -4;
const REGISTER_COUNT: u32 = 13;

// Keep both the dispatch nesting and the worst-case function body bounded. A
// PolkaVM block can be arbitrarily long; splitting it must not add gas charges.
const BLOCKS_PER_FUNCTION: usize = 128;
const INSTRUCTIONS_PER_BLOCK: usize = 16;
const RESOLVERS_PER_FUNCTION: usize = 128;
// Bound each native compilation unit without splitting a block-group function.
const CODE_PART_BYTES: usize = 8 * 1024 * 1024;
const CODE_PART_SECTION: &str = "epoca.pvm.code-part";

const TYPE_BLOCK: u32 = 0;
const TYPE_BINARY_I64: u32 = 1;
const TYPE_BEGIN: u32 = 2;
const TYPE_SET_GAS: u32 = 3;
const TYPE_UNARY_I64: u32 = 4;
const TYPE_RESOLVER: u32 = 5;
const TYPE_INDIRECT: u32 = 6;
const TYPE_LOAD: u32 = 7;

const GLOBAL_PC: u32 = REGISTER_COUNT;
const GLOBAL_GAS: u32 = GLOBAL_PC + 1;
const GLOBAL_HEAP_SIZE: u32 = GLOBAL_GAS + 1;
const GLOBAL_ECALL: u32 = GLOBAL_HEAP_SIZE + 1;
const GLOBAL_TRAP_PC: u32 = GLOBAL_ECALL + 1;

const LOCAL_ADDR: u32 = 0;
const LOCAL_PHYS: u32 = 1;
const LOCAL_I64_0: u32 = 2;
const LOCAL_I64_1: u32 = 3;

#[derive(Clone, Copy)]
struct Layout {
    ro_address: u32,
    ro_size: u32,
    ro_phys: u32,
    rw_address: u32,
    rw_size: u32,
    rw_phys: u32,
    heap_base: u32,
    heap_limit: u32,
    stack_low: u32,
    stack_high: u32,
    stack_phys: u32,
    rw_pages: u64,
    rw_max_pages: u64,
}

#[derive(Clone, Copy)]
enum LoadKind {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
}

impl LoadKind {
    // Declaration order is also the offset in the shared helper function range.
    const ALL: [Self; 7] = [
        Self::U8,
        Self::I8,
        Self::U16,
        Self::I16,
        Self::U32,
        Self::I32,
        Self::U64,
    ];
}

#[derive(Clone, Copy)]
enum StoreKind {
    U8,
    U16,
    U32,
    U64,
}

pub fn translate(program: &[u8]) -> Result<Vec<u8>> {
    translate_with_part_limit(program, None)
}

pub fn translate_partitioned(program: &[u8]) -> Result<Vec<u8>> {
    translate_with_part_limit(program, Some(CODE_PART_BYTES))
}

fn translate_with_part_limit(program: &[u8], part_limit: Option<usize>) -> Result<Vec<u8>> {
    let partitioned = part_limit.is_some();
    if program.is_empty() || program.len() > MAX_PROGRAM_BYTES {
        bail!("guest program exceeds browser limit");
    }
    let blob =
        ProgramBlob::parse(program.into()).context("parse PolkaVM program for Wasm translation")?;
    blob.validate_code_with_isa(blob.isa())
        .map_err(|pc| anyhow!("invalid PolkaVM instruction at {pc}"))?;
    if blob.stack_size() > MAX_GUEST_STACK_BYTES {
        bail!("guest stack exceeds browser limit");
    }
    if blob.rw_data_size() > MAX_GUEST_RW_DATA_BYTES {
        bail!("guest read-write data exceeds browser limit");
    }

    let instructions: Vec<_> = blob.instructions().collect();
    if instructions.is_empty() {
        bail!("PolkaVM program contains no instructions");
    }
    let metered_targets = collect_metered_targets(&instructions);

    let layout = build_layout(&blob)?;
    let targets = collect_block_targets(&blob, &instructions)?;
    let (blocks, block_by_pc) = build_blocks(&instructions, &targets)?;
    let jump_targets = blob.jump_table();
    let block_group_count = blocks.len().div_ceil(BLOCKS_PER_FUNCTION) as u32;
    let resolver_group_count = jump_targets.len().div_ceil(RESOLVERS_PER_FUNCTION as u32);

    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], [ValType::I32]);
    types
        .ty()
        .function([ValType::I64, ValType::I64], [ValType::I64]);
    types
        .ty()
        .function([ValType::I32, ValType::I64], [ValType::I32]);
    types.ty().function([ValType::I64], []);
    types.ty().function([ValType::I64], [ValType::I64]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types
        .ty()
        .function([ValType::I32, ValType::I32, ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I64]);
    module.section(&types);

    let load_function_base = 7;
    let helper_count = load_function_base + LoadKind::ALL.len() as u32;
    let root_helper_count = if partitioned { 0 } else { helper_count };
    let root_block_count = if partitioned { 0 } else { block_group_count };
    let resolver_base = root_helper_count + root_block_count;
    let dispatcher_index = resolver_base + resolver_group_count;
    let begin_index = dispatcher_index + 1;
    let set_gas_index = begin_index + 1;

    let mut functions = FunctionSection::new();
    let helper_types = [
        TYPE_BINARY_I64,
        TYPE_UNARY_I64,
        TYPE_UNARY_I64,
        TYPE_INDIRECT,
        TYPE_INDIRECT,
        TYPE_RESOLVER,
        TYPE_BLOCK,
    ]
    .into_iter()
    .chain(std::iter::repeat_n(TYPE_LOAD, LoadKind::ALL.len()))
    .collect::<Vec<_>>();
    let mut part_imports = ImportSection::new();
    if !partitioned {
        for &ty in &helper_types {
            functions.function(ty);
        }
        for _ in 0..block_group_count {
            functions.function(TYPE_BLOCK);
        }
    }
    for _ in 0..resolver_group_count {
        functions.function(TYPE_RESOLVER);
    }
    functions.function(TYPE_BLOCK);
    functions.function(TYPE_BEGIN);
    functions.function(TYPE_SET_GAS);
    module.section(&functions);

    let mut table = TableSection::new();
    let table_type = TableType {
        element_type: RefType::FUNCREF,
        minimum: u64::from(block_group_count + resolver_group_count),
        maximum: Some(u64::from(block_group_count + resolver_group_count)),
        table64: false,
        shared: false,
    };
    table.table(table_type);
    part_imports.import("pvm", "__table", EntityType::Table(table_type));
    module.section(&table);

    let rw_prefix_pages = u64::from(layout.rw_phys / PAGE_SIZE);
    let mut memory = MemorySection::new();
    let memory_type = MemoryType {
        minimum: rw_prefix_pages + layout.rw_pages,
        maximum: Some(rw_prefix_pages + layout.rw_max_pages),
        memory64: false,
        shared: false,
        page_size_log2: None,
    };
    memory.memory(memory_type);
    part_imports.import("pvm", "memory", EntityType::Memory(memory_type));
    module.section(&memory);

    let mut globals = GlobalSection::new();
    for index in 0..REGISTER_COUNT {
        globals.global(
            shared_global_type(index),
            &ConstExpr::i64_const(if index == Reg::SP.to_u32() {
                layout.stack_high as i64
            } else {
                0
            }),
        );
    }
    for _ in 0..5 {
        let index = globals.len();
        let ty = shared_global_type(index);
        let initial = if ty.val_type == ValType::I64 {
            ConstExpr::i64_const(0)
        } else {
            ConstExpr::i32_const(0)
        };
        globals.global(ty, &initial);
    }
    module.section(&globals);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("__table", ExportKind::Table, 0);
    for index in 0..globals.len() {
        let name = format!("__global{index}");
        exports.export(&name, ExportKind::Global, index);
        part_imports.import("pvm", &name, EntityType::Global(shared_global_type(index)));
    }
    exports.export("pvm_begin", ExportKind::Func, begin_index);
    exports.export("pvm_resume", ExportKind::Func, dispatcher_index);
    exports.export("pvm_set_gas", ExportKind::Func, set_gas_index);
    for reg in Reg::ALL {
        exports.export(reg.name_non_abi(), ExportKind::Global, reg.to_u32());
    }
    exports.export("pc", ExportKind::Global, GLOBAL_PC);
    exports.export("gas", ExportKind::Global, GLOBAL_GAS);
    exports.export("heap_size", ExportKind::Global, GLOBAL_HEAP_SIZE);
    exports.export("ecall", ExportKind::Global, GLOBAL_ECALL);
    exports.export("trap_pc", ExportKind::Global, GLOBAL_TRAP_PC);
    module.section(&exports);

    let table_functions: Vec<u32> = (root_helper_count..dispatcher_index).collect();
    let mut elements = ElementSection::new();
    elements.active(
        Some(0),
        &ConstExpr::i32_const(if partitioned {
            block_group_count as i32
        } else {
            0
        }),
        Elements::Functions(Cow::Owned(table_functions)),
    );
    module.section(&elements);

    let mut helper_code = CodeSection::new();
    helper_code.function(&emit_mulhu());
    helper_code.function(&emit_bswap32());
    helper_code.function(&emit_bswap64());

    let context = EmitContext {
        layout,
        block_by_pc: &block_by_pc,
        jump_table_len: jump_targets.len(),
        resolver_table_base: block_group_count,
        control_targets: &targets,
        mulhu_function: 0,
        bswap32_function: 1,
        metered_targets: &metered_targets,
        bswap64_function: 2,
        is_64_bit: blob.is_64_bit(),
        indirect_function: 3,
        return_function: 4,
        physical_address_function: 5,
        gas_function: 6,
        load_function_base,
        local_block_start: 0,
        local_block_end: root_block_count,
    };
    helper_code.function(&emit_indirect_helper(&context, true));
    helper_code.function(&emit_indirect_helper(&context, false));
    helper_code.function(&emit_physical_address_helper(layout));
    helper_code.function(&emit_gas_helper());
    for kind in LoadKind::ALL {
        helper_code.function(&emit_load_helper(kind, context.physical_address_function));
    }
    let mut code = if partitioned {
        CodeSection::new()
    } else {
        let mut code = helper_code.clone();
        for group in blocks.chunks(BLOCKS_PER_FUNCTION) {
            code.function(&emit_block_group(&context, group)?);
        }
        code
    };
    for start in (0..jump_targets.len()).step_by(RESOLVERS_PER_FUNCTION) {
        let count = (jump_targets.len() - start).min(RESOLVERS_PER_FUNCTION as u32);
        let mut function = Function::new([]);
        emit_switch(&mut function, count as usize, 0, RESOLVERS_PER_FUNCTION);
        for index in start..start + count {
            let target = jump_targets
                .get_by_index(index)
                .expect("jump table index is in bounds");
            function.instruction(&W::End);
            function.instruction(&W::I32Const(
                block_by_pc
                    .get(target.0)
                    .expect("validated jump target has a block") as i32,
            ));
            function.instruction(&W::Return);
        }
        function.instruction(&W::End);
        function.instruction(&W::Unreachable);
        function.instruction(&W::End);
        code.function(&function);
    }
    code.function(&emit_dispatcher(blocks.len() as u32));
    code.function(&emit_begin(dispatcher_index));
    code.function(&emit_set_gas());
    module.section(&code);

    let mut data = DataSection::new();
    if !blob.ro_data().is_empty() {
        data.active(
            0,
            &ConstExpr::i32_const(layout.ro_phys as i32),
            blob.ro_data().iter().copied(),
        );
    }
    if !blob.rw_data().is_empty() {
        data.active(
            0,
            &ConstExpr::i32_const(layout.rw_phys as i32),
            blob.rw_data().iter().copied(),
        );
    }
    module.section(&data);

    let metadata = encode_metadata(&blob, &block_by_pc, layout)?;
    module.section(&CustomSection {
        name: Cow::Borrowed("epoca.pvm.meta"),
        data: Cow::Owned(metadata),
    });

    if let Some(part_limit) = part_limit {
        let mut part_code = helper_code.clone();
        let mut part_table_offset = 0;
        for (group_index, group) in blocks.chunks(BLOCKS_PER_FUNCTION).enumerate() {
            let mut part_context = EmitContext {
                local_block_start: part_table_offset,
                local_block_end: group_index as u32 + 1,
                ..context
            };
            let mut function = emit_block_group(&part_context, group)?;
            // Reserve the maximum u32 body-size prefix as well as the actual body.
            if part_code.len() > helper_count
                && part_code.byte_len() + function.byte_len() + 5 > part_limit
            {
                append_code_part(
                    &mut module,
                    &types,
                    &part_imports,
                    &part_code,
                    &helper_types,
                    part_table_offset,
                );
                part_table_offset += part_code.len() - helper_count;
                part_code = helper_code.clone();
                part_context.local_block_start = part_table_offset;
                function = emit_block_group(&part_context, group)?;
            }
            part_code.function(&function);
        }
        if part_code.len() > helper_count {
            append_code_part(
                &mut module,
                &types,
                &part_imports,
                &part_code,
                &helper_types,
                part_table_offset,
            );
        }
    }
    Ok(module.finish())
}

fn shared_global_type(index: u32) -> GlobalType {
    GlobalType {
        val_type: if index < REGISTER_COUNT || index == GLOBAL_GAS || index == GLOBAL_HEAP_SIZE {
            ValType::I64
        } else {
            ValType::I32
        },
        mutable: true,
        shared: false,
    }
}

fn append_code_part(
    root: &mut Module,
    types: &TypeSection,
    imports: &ImportSection,
    code: &CodeSection,
    helper_types: &[u32],
    table_offset: u32,
) {
    let mut part = Module::new();
    part.section(types);
    part.section(imports);
    let mut functions = FunctionSection::new();
    let helper_count = helper_types.len() as u32;
    for &ty in helper_types {
        functions.function(ty);
    }
    for _ in helper_count..code.len() {
        functions.function(TYPE_BLOCK);
    }
    part.section(&functions);
    let mut elements = ElementSection::new();
    elements.active(
        Some(0),
        &ConstExpr::i32_const(table_offset as i32),
        Elements::Functions(Cow::Owned((helper_count..code.len()).collect())),
    );
    part.section(&elements);
    part.section(code);
    root.section(&CustomSection {
        name: Cow::Borrowed(CODE_PART_SECTION),
        data: Cow::Owned(part.finish()),
    });
}

fn align(value: u32, alignment: u32) -> Result<u32> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| anyhow!("translated memory layout overflow"))
}

fn build_layout(blob: &ProgramBlob) -> Result<Layout> {
    let map = MemoryMapBuilder::new(PAGE_SIZE)
        .ro_data_size(blob.ro_data_size())
        .rw_data_size(blob.rw_data_size())
        .stack_size(blob.stack_size())
        .build()
        .map_err(|error| anyhow!(error))?;
    let heap_limit = MAX_GUEST_HEAP_BYTES.min(map.max_heap_size());
    let rw_max_bytes = map
        .heap_base()
        .wrapping_sub(map.rw_data_address())
        .checked_add(heap_limit)
        .ok_or_else(|| anyhow!("translated heap layout overflow"))?;
    let pages = |bytes: u32| align(bytes, PAGE_SIZE).map(|bytes| u64::from(bytes / PAGE_SIZE));
    let ro_pages = pages(map.ro_data_size())?;
    let rw_pages = pages(map.rw_data_size())?;
    let rw_max_pages = pages(rw_max_bytes)?;
    let stack_pages = pages(map.stack_size())?;
    let stack_phys = u32::try_from(
        ro_pages
            .checked_mul(u64::from(PAGE_SIZE))
            .ok_or_else(|| anyhow!("translated stack layout overflow"))?,
    )
    .map_err(|_| anyhow!("translated stack layout exceeds wasm32"))?;
    let rw_phys = u32::try_from(
        ro_pages
            .checked_add(stack_pages)
            .and_then(|pages| pages.checked_mul(u64::from(PAGE_SIZE)))
            .ok_or_else(|| anyhow!("translated read-write layout overflow"))?,
    )
    .map_err(|_| anyhow!("translated read-write layout exceeds wasm32"))?;
    Ok(Layout {
        ro_address: map.ro_data_address(),
        ro_size: map.ro_data_size(),
        ro_phys: 0,
        rw_address: map.rw_data_address(),
        rw_size: map.rw_data_size(),
        rw_phys,
        heap_base: map.heap_base(),
        heap_limit,
        stack_low: map.stack_address_low(),
        stack_high: map.stack_address_high(),
        stack_phys,
        rw_pages,
        rw_max_pages,
    })
}

fn is_terminator(kind: PvmInstruction) -> bool {
    matches!(
        kind,
        PvmInstruction::trap
            | PvmInstruction::jump(..)
            | PvmInstruction::jump_indirect(..)
            | PvmInstruction::load_imm_and_jump(..)
            | PvmInstruction::load_imm_and_jump_indirect(..)
            | PvmInstruction::branch_eq(..)
            | PvmInstruction::branch_not_eq(..)
            | PvmInstruction::branch_less_unsigned(..)
            | PvmInstruction::branch_less_signed(..)
            | PvmInstruction::branch_greater_or_equal_unsigned(..)
            | PvmInstruction::branch_greater_or_equal_signed(..)
            | PvmInstruction::branch_eq_imm(..)
            | PvmInstruction::branch_not_eq_imm(..)
            | PvmInstruction::branch_less_unsigned_imm(..)
            | PvmInstruction::branch_less_signed_imm(..)
            | PvmInstruction::branch_greater_or_equal_unsigned_imm(..)
            | PvmInstruction::branch_greater_or_equal_signed_imm(..)
            | PvmInstruction::branch_less_or_equal_signed_imm(..)
            | PvmInstruction::branch_less_or_equal_unsigned_imm(..)
            | PvmInstruction::branch_greater_signed_imm(..)
            | PvmInstruction::branch_greater_unsigned_imm(..)
            | PvmInstruction::ecalli(..)
    )
}

fn direct_target(kind: PvmInstruction) -> Option<u32> {
    match kind {
        PvmInstruction::jump(target) | PvmInstruction::load_imm_and_jump(_, _, target) => {
            Some(target)
        }
        PvmInstruction::branch_eq(_, _, target)
        | PvmInstruction::branch_not_eq(_, _, target)
        | PvmInstruction::branch_less_unsigned(_, _, target)
        | PvmInstruction::branch_less_signed(_, _, target)
        | PvmInstruction::branch_greater_or_equal_unsigned(_, _, target)
        | PvmInstruction::branch_greater_or_equal_signed(_, _, target)
        | PvmInstruction::branch_eq_imm(_, _, target)
        | PvmInstruction::branch_not_eq_imm(_, _, target)
        | PvmInstruction::branch_less_unsigned_imm(_, _, target)
        | PvmInstruction::branch_less_signed_imm(_, _, target)
        | PvmInstruction::branch_greater_or_equal_unsigned_imm(_, _, target)
        | PvmInstruction::branch_greater_or_equal_signed_imm(_, _, target)
        | PvmInstruction::branch_less_or_equal_signed_imm(_, _, target)
        | PvmInstruction::branch_less_or_equal_unsigned_imm(_, _, target)
        | PvmInstruction::branch_greater_signed_imm(_, _, target)
        | PvmInstruction::branch_greater_unsigned_imm(_, _, target) => Some(target),
        _ => None,
    }
}

fn collect_block_targets(
    blob: &ProgramBlob,
    instructions: &[ParsedInstruction],
) -> Result<Vec<u32>> {
    // Flat sorted indexes avoid a tree node and a second instruction allocation
    // for each of the millions of blocks in a large guest.
    let mut targets = vec![instructions[0].offset.0];
    targets.extend(blob.exports().map(|export| export.program_counter().0));
    targets.extend(blob.jump_table().into_iter().map(|target| target.0));
    for instruction in instructions {
        if let Some(target) = direct_target(instruction.kind) {
            targets.push(target);
        }
        if is_terminator(instruction.kind)
            && instruction.next_offset.0 <= instructions.last().unwrap().offset.0
        {
            targets.push(instruction.next_offset.0);
        }
    }
    targets.sort_unstable();
    targets.dedup();
    for &target in &targets {
        if instructions
            .binary_search_by_key(&target, |instruction| instruction.offset.0)
            .is_err()
        {
            bail!("PolkaVM control-flow target {target} is not an instruction");
        }
    }
    Ok(targets)
}

struct BlockMap {
    pcs: Vec<u32>,
}

impl BlockMap {
    fn get(&self, pc: u32) -> Option<u32> {
        self.pcs.binary_search(&pc).ok().map(|index| index as u32)
    }
}

type BlockLayout<'a> = (Vec<&'a [ParsedInstruction]>, BlockMap);

fn build_blocks<'a>(
    instructions: &'a [ParsedInstruction],
    targets: &[u32],
) -> Result<BlockLayout<'a>> {
    let mut blocks = Vec::new();
    let mut pcs = Vec::new();
    let mut start = 0;
    let mut next_target = 1;
    for index in 1..instructions.len() {
        let is_target = targets.get(next_target) == Some(&instructions[index].offset.0);
        if is_target {
            next_target += 1;
        }
        if is_target || index - start == INSTRUCTIONS_PER_BLOCK {
            pcs.push(instructions[start].offset.0);
            blocks.push(&instructions[start..index]);
            start = index;
        }
    }
    pcs.push(instructions[start].offset.0);
    blocks.push(&instructions[start..]);
    Ok((blocks, BlockMap { pcs }))
}

fn collect_metered_targets(instructions: &[ParsedInstruction]) -> Vec<u32> {
    let mut targets = Vec::new();
    for instruction in instructions {
        if let Some(target) = direct_target(instruction.kind) {
            if target <= instruction.offset.0 {
                targets.push(target);
            }
        }
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

#[derive(Clone, Copy)]
struct EmitContext<'a> {
    layout: Layout,
    block_by_pc: &'a BlockMap,
    jump_table_len: u32,
    resolver_table_base: u32,
    control_targets: &'a [u32],
    mulhu_function: u32,
    bswap32_function: u32,
    metered_targets: &'a [u32],
    bswap64_function: u32,
    is_64_bit: bool,
    indirect_function: u32,
    return_function: u32,
    physical_address_function: u32,
    gas_function: u32,
    load_function_base: u32,
    local_block_start: u32,
    local_block_end: u32,
}

fn reg_index(reg: RawReg) -> u32 {
    reg.get().to_u32()
}

fn memarg(bytes: u32, memory_index: u32) -> MemArg {
    MemArg {
        offset: 0,
        align: bytes.trailing_zeros(),
        memory_index,
    }
}

fn emit_mulhu() -> Function {
    let mut f = Function::new([(4, ValType::I64)]);
    // Hacker's Delight 32-bit limb multiplication.
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::I64Const(0xffff_ffff));
    f.instruction(&W::I64And);
    f.instruction(&W::LocalSet(2));
    f.instruction(&W::LocalGet(1));
    f.instruction(&W::I64Const(0xffff_ffff));
    f.instruction(&W::I64And);
    f.instruction(&W::LocalSet(3));
    f.instruction(&W::LocalGet(2));
    f.instruction(&W::LocalGet(3));
    f.instruction(&W::I64Mul);
    f.instruction(&W::I64Const(32));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::I64Const(32));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::LocalGet(3));
    f.instruction(&W::I64Mul);
    f.instruction(&W::I64Add);
    f.instruction(&W::LocalTee(4));
    f.instruction(&W::I64Const(32));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::LocalSet(5));
    f.instruction(&W::LocalGet(4));
    f.instruction(&W::I64Const(0xffff_ffff));
    f.instruction(&W::I64And);
    f.instruction(&W::LocalGet(2));
    f.instruction(&W::LocalGet(1));
    f.instruction(&W::I64Const(32));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::I64Mul);
    f.instruction(&W::I64Add);
    f.instruction(&W::I64Const(32));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::LocalGet(5));
    f.instruction(&W::I64Add);
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::I64Const(32));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::LocalGet(1));
    f.instruction(&W::I64Const(32));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::I64Mul);
    f.instruction(&W::I64Add);
    f.instruction(&W::End);
    f
}

fn emit_bswap32() -> Function {
    let mut f = Function::new([]);
    for (mask, shift, left) in [
        (0x0000_00ffu32, 24, true),
        (0x0000_ff00u32, 8, true),
        (0x00ff_0000u32, 8, false),
        (0xff00_0000u32, 24, false),
    ] {
        f.instruction(&W::LocalGet(0));
        f.instruction(&W::I32WrapI64);
        f.instruction(&W::I32Const(mask as i32));
        f.instruction(&W::I32And);
        f.instruction(&W::I32Const(shift));
        f.instruction(if left { &W::I32Shl } else { &W::I32ShrU });
        if mask != 0x0000_00ff {
            f.instruction(&W::I32Or);
        }
    }
    f.instruction(&W::I64ExtendI32U);
    f.instruction(&W::End);
    f
}

fn emit_bswap64() -> Function {
    let mut f = Function::new([]);
    for (mask, left, right) in [
        (0x00ff_00ff_00ff_00ffu64, 8, 8),
        (0x0000_ffff_0000_ffffu64, 16, 16),
        (0x0000_0000_ffff_ffffu64, 32, 32),
    ] {
        f.instruction(&W::LocalGet(0));
        f.instruction(&W::I64Const(mask as i64));
        f.instruction(&W::I64And);
        f.instruction(&W::I64Const(left));
        f.instruction(&W::I64Shl);
        f.instruction(&W::LocalGet(0));
        f.instruction(&W::I64Const(!mask as i64));
        f.instruction(&W::I64And);
        f.instruction(&W::I64Const(right));
        f.instruction(&W::I64ShrU);
        f.instruction(&W::I64Or);
        if left != 32 {
            f.instruction(&W::LocalSet(0));
        }
    }
    f.instruction(&W::End);
    f
}

fn emit_dispatcher(block_count: u32) -> Function {
    let mut f = Function::new([]);
    f.instruction(&W::GlobalGet(GLOBAL_PC));
    f.instruction(&W::I32Const(block_count as i32));
    f.instruction(&W::I32GeU);
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::I32Const(STATUS_TRAP));
    f.instruction(&W::Return);
    f.instruction(&W::End);
    emit_dispatch_from_pc(&mut f);
    f.instruction(&W::End);
    f
}

fn emit_dispatch_from_pc(f: &mut Function) {
    f.instruction(&W::GlobalGet(GLOBAL_PC));
    f.instruction(&W::I32Const(BLOCKS_PER_FUNCTION.trailing_zeros() as i32));
    f.instruction(&W::I32ShrU);
    f.instruction(&W::ReturnCallIndirect {
        type_index: TYPE_BLOCK,
        table_index: 0,
    });
}

fn emit_begin(dispatcher_index: u32) -> Function {
    let mut f = Function::new([]);
    f.instruction(&W::I64Const(RETURN_TO_HOST as i64));
    f.instruction(&W::GlobalSet(Reg::RA.to_u32()));
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::GlobalSet(GLOBAL_PC));
    f.instruction(&W::LocalGet(1));
    f.instruction(&W::GlobalSet(GLOBAL_GAS));
    f.instruction(&W::ReturnCall(dispatcher_index));
    f.instruction(&W::End);
    f
}

fn emit_set_gas() -> Function {
    let mut f = Function::new([]);
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::GlobalSet(GLOBAL_GAS));
    f.instruction(&W::End);
    f
}

fn encode_metadata(blob: &ProgramBlob, block_by_pc: &BlockMap, layout: Layout) -> Result<Vec<u8>> {
    let mut bytes = b"EPM2".to_vec();
    bytes.extend_from_slice(&u32::from(blob.is_64_bit()).to_le_bytes());
    for value in [
        layout.ro_address,
        layout.ro_size,
        layout.ro_phys,
        layout.rw_address,
        layout.rw_size,
        layout.rw_phys,
        layout.heap_base,
        layout.heap_limit,
        layout.stack_low,
        layout.stack_high,
        layout.stack_phys,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let imports: Vec<_> = blob.imports().into_iter().collect();
    bytes.extend_from_slice(&(imports.len() as u32).to_le_bytes());
    for import in imports {
        match import {
            Some(symbol) => {
                let name = symbol.as_bytes();
                let length =
                    u16::try_from(name.len()).context("PolkaVM import name is too long")?;
                bytes.extend_from_slice(&length.to_le_bytes());
                bytes.extend_from_slice(name);
            }
            None => bytes.extend_from_slice(&0u16.to_le_bytes()),
        }
    }
    let exports: Vec<_> = blob.exports().collect();
    bytes.extend_from_slice(&(exports.len() as u32).to_le_bytes());
    for export in exports {
        let name = export.symbol().as_bytes();
        let length = u16::try_from(name.len()).context("PolkaVM export name is too long")?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(name);
        let block = block_by_pc
            .get(export.program_counter().0)
            .ok_or_else(|| anyhow!("export target is not a translated block"))?;
        bytes.extend_from_slice(&block.to_le_bytes());
    }
    Ok(bytes)
}

fn emit_gas_helper() -> Function {
    let mut f = Function::new([(1, ValType::I64)]);
    f.instruction(&W::GlobalGet(GLOBAL_GAS));
    f.instruction(&W::I64Const(1));
    f.instruction(&W::I64Sub);
    f.instruction(&W::LocalTee(0));
    f.instruction(&W::GlobalSet(GLOBAL_GAS));
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::I64Const(0));
    f.instruction(&W::I64LeS);
    f.instruction(&W::End);
    f
}

fn emit_gas_charge(f: &mut Function, gas_function: u32) {
    f.instruction(&W::Call(gas_function));
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::I32Const(STATUS_OUT_OF_GAS));
    f.instruction(&W::Return);
    f.instruction(&W::End);
}

// Emit a bounded br_table switch. The extra outer label is the invalid selector
// path; callers emit one End and one terminating case per entry, then close it.
fn emit_switch(f: &mut Function, count: usize, local: u32, capacity: usize) {
    for _ in 0..=count {
        f.instruction(&W::Block(BlockType::Empty));
    }
    f.instruction(&W::LocalGet(local));
    f.instruction(&W::I32Const((capacity - 1) as i32));
    f.instruction(&W::I32And);
    let labels: Vec<u32> = (0..count as u32).collect();
    f.instruction(&W::BrTable(Cow::Owned(labels), count as u32));
}

fn emit_block_group(
    context: &EmitContext<'_>,
    blocks: &[&[ParsedInstruction]],
) -> Result<Function> {
    let mut f = Function::new([(2, ValType::I32), (2, ValType::I64)]);
    f.instruction(&W::GlobalGet(GLOBAL_PC));
    f.instruction(&W::LocalSet(LOCAL_ADDR));
    emit_switch(&mut f, blocks.len(), LOCAL_ADDR, BLOCKS_PER_FUNCTION);
    for block in blocks {
        f.instruction(&W::End);
        if context
            .metered_targets
            .binary_search(&block[0].offset.0)
            .is_ok()
        {
            emit_gas_charge(&mut f, context.gas_function);
        }
        let mut terminated = false;
        for instruction in *block {
            if emit_instruction(context, &mut f, instruction)? {
                terminated = true;
                break;
            }
        }
        if !terminated {
            emit_block_target(context, &mut f, block.last().unwrap().next_offset.0)?;
        }
    }
    f.instruction(&W::End);
    f.instruction(&W::Unreachable);
    f.instruction(&W::End);
    Ok(f)
}

fn emit_block_target(context: &EmitContext<'_>, f: &mut Function, pc: u32) -> Result<()> {
    let target = context
        .block_by_pc
        .get(pc)
        .ok_or_else(|| anyhow!("translated fallthrough target {pc} is missing"))?;
    f.instruction(&W::I32Const(target as i32));
    f.instruction(&W::GlobalSet(GLOBAL_PC));
    let group = target / BLOCKS_PER_FUNCTION as u32;
    if (context.local_block_start..context.local_block_end).contains(&group) {
        let helper_count = context.load_function_base + LoadKind::ALL.len() as u32;
        f.instruction(&W::ReturnCall(
            helper_count + group - context.local_block_start,
        ));
    } else {
        f.instruction(&W::I32Const(group as i32));
        f.instruction(&W::ReturnCallIndirect {
            type_index: TYPE_BLOCK,
            table_index: 0,
        });
    }
    Ok(())
}

fn emit_return_target(context: &EmitContext<'_>, f: &mut Function, pc: u32) -> Result<()> {
    emit_block_target(context, f, pc)
}

fn emit_trap(f: &mut Function, pc: u32) {
    f.instruction(&W::I32Const(pc as i32));
    f.instruction(&W::GlobalSet(GLOBAL_TRAP_PC));
    f.instruction(&W::I32Const(STATUS_TRAP));
    f.instruction(&W::Return);
}

fn emit_reg(f: &mut Function, reg: RawReg) {
    f.instruction(&W::GlobalGet(reg_index(reg)));
}

fn emit_set_reg(f: &mut Function, reg: RawReg) {
    f.instruction(&W::GlobalSet(reg_index(reg)));
}

fn emit_i32_result(f: &mut Function, reg: RawReg, operation: W<'_>) {
    f.instruction(&operation);
    f.instruction(&W::I64ExtendI32S);
    emit_set_reg(f, reg);
}

fn emit_address(f: &mut Function, base: Option<RawReg>, offset: i32) {
    if let Some(base) = base {
        emit_reg(f, base);
        f.instruction(&W::I32WrapI64);
        if offset != 0 {
            f.instruction(&W::I32Const(offset));
            f.instruction(&W::I32Add);
        }
    } else {
        f.instruction(&W::I32Const(offset));
    }
}

fn static_target(layout: Layout, address: u32, bytes: u32, write: bool) -> Option<u32> {
    let end = u64::from(address).checked_add(u64::from(bytes))?;
    let (virtual_base, physical_base) = if !write
        && address >= layout.ro_address
        && end <= u64::from(layout.ro_address) + u64::from(layout.ro_size)
    {
        (layout.ro_address, layout.ro_phys)
    } else if address >= layout.rw_address
        && end <= u64::from(layout.rw_address) + u64::from(layout.rw_size)
    {
        (layout.rw_address, layout.rw_phys)
    } else if address >= layout.stack_low && end <= u64::from(layout.stack_high) {
        (layout.stack_low, layout.stack_phys)
    } else {
        return None;
    };
    physical_base.checked_add(address.checked_sub(virtual_base)?)
}

fn emit_physical_address(f: &mut Function, virtual_base: u32, physical_base: u32) {
    let offset = physical_base.wrapping_sub(virtual_base) as i32;
    if offset != 0 {
        f.instruction(&W::I32Const(offset));
        f.instruction(&W::I32Add);
    }
}

fn emit_physical_address_helper(layout: Layout) -> Function {
    let mut f = Function::new([]);
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::I32Const(layout.stack_low as i32));
    f.instruction(&W::I32GeU);
    f.instruction(&W::If(BlockType::Result(ValType::I32)));
    f.instruction(&W::LocalGet(0));
    emit_physical_address(&mut f, layout.stack_low, layout.stack_phys);
    f.instruction(&W::Else);
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::I32Const(layout.rw_address as i32));
    f.instruction(&W::I32GeU);
    f.instruction(&W::If(BlockType::Result(ValType::I32)));
    f.instruction(&W::LocalGet(0));
    emit_physical_address(&mut f, layout.rw_address, layout.rw_phys);
    f.instruction(&W::Else);
    f.instruction(&W::LocalGet(0));
    emit_physical_address(&mut f, layout.ro_address, layout.ro_phys);
    f.instruction(&W::End);
    f.instruction(&W::End);
    f.instruction(&W::End);
    f
}

fn emit_load_helper(kind: LoadKind, physical_address_function: u32) -> Function {
    let mut f = Function::new([]);
    f.instruction(&W::LocalGet(0));
    f.instruction(&W::Call(physical_address_function));
    emit_load_at(&mut f, kind);
    f.instruction(&W::End);
    f
}

fn emit_load_at(f: &mut Function, kind: LoadKind) {
    let bytes = match kind {
        LoadKind::U8 | LoadKind::I8 => 1,
        LoadKind::U16 | LoadKind::I16 => 2,
        LoadKind::U32 | LoadKind::I32 => 4,
        LoadKind::U64 => 8,
    };
    f.instruction(&match kind {
        LoadKind::U8 => W::I64Load8U(memarg(bytes, 0)),
        LoadKind::I8 => W::I64Load8S(memarg(bytes, 0)),
        LoadKind::U16 => W::I64Load16U(memarg(bytes, 0)),
        LoadKind::I16 => W::I64Load16S(memarg(bytes, 0)),
        LoadKind::U32 => W::I64Load32U(memarg(bytes, 0)),
        LoadKind::I32 => W::I64Load32S(memarg(bytes, 0)),
        LoadKind::U64 => W::I64Load(memarg(bytes, 0)),
    });
}

fn emit_load(
    context: &EmitContext<'_>,
    f: &mut Function,
    pc: u32,
    dst: RawReg,
    base: Option<RawReg>,
    offset: i32,
    kind: LoadKind,
) {
    let bytes = match kind {
        LoadKind::U8 | LoadKind::I8 => 1,
        LoadKind::U16 | LoadKind::I16 => 2,
        LoadKind::U32 | LoadKind::I32 => 4,
        LoadKind::U64 => 8,
    };
    if base.is_none() {
        if let Some(physical) = static_target(context.layout, offset as u32, bytes, false) {
            f.instruction(&W::I32Const(physical as i32));
            emit_load_at(f, kind);
        } else {
            emit_trap(f, pc);
        }
        emit_set_reg(f, dst);
        return;
    }
    if base == Some(Reg::SP.raw()) {
        let offset = offset
            .wrapping_sub(context.layout.stack_low as i32)
            .wrapping_add(context.layout.stack_phys as i32);
        emit_address(f, base, offset);
        emit_load_at(f, kind);
        emit_set_reg(f, dst);
        return;
    }
    emit_address(f, base, offset);
    f.instruction(&W::Call(context.load_function_base + kind as u32));
    emit_set_reg(f, dst);
}

fn emit_store_value(f: &mut Function, kind: StoreKind, source: Option<RawReg>, immediate: i32) {
    if let Some(source) = source {
        emit_reg(f, source);
    } else {
        f.instruction(&W::I64Const(immediate as i64));
    }
    f.instruction(&match kind {
        StoreKind::U8 => W::I64Store8(memarg(1, 0)),
        StoreKind::U16 => W::I64Store16(memarg(2, 0)),
        StoreKind::U32 => W::I64Store32(memarg(4, 0)),
        StoreKind::U64 => W::I64Store(memarg(8, 0)),
    });
}

// The decoded store operands map directly to the PolkaVM instruction fields;
// grouping them would only move this argument list into a transient struct.
#[allow(clippy::too_many_arguments)]
fn emit_store(
    context: &EmitContext<'_>,
    f: &mut Function,
    pc: u32,
    source: Option<RawReg>,
    base: Option<RawReg>,
    offset: i32,
    immediate: i32,
    kind: StoreKind,
) {
    let bytes = match kind {
        StoreKind::U8 => 1,
        StoreKind::U16 => 2,
        StoreKind::U32 => 4,
        StoreKind::U64 => 8,
    };
    if base.is_none() {
        if let Some(physical) = static_target(context.layout, offset as u32, bytes, true) {
            f.instruction(&W::I32Const(physical as i32));
            emit_store_value(f, kind, source, immediate);
        } else {
            emit_trap(f, pc);
        }
        return;
    }
    if base == Some(Reg::SP.raw()) {
        let offset = offset
            .wrapping_sub(context.layout.stack_low as i32)
            .wrapping_add(context.layout.stack_phys as i32);
        emit_address(f, base, offset);
        emit_store_value(f, kind, source, immediate);
        return;
    }
    emit_address(f, base, offset);
    f.instruction(&W::LocalTee(LOCAL_ADDR));
    f.instruction(&W::I32Const(context.layout.rw_address as i32));
    f.instruction(&W::I32LtU);
    f.instruction(&W::If(BlockType::Empty));
    emit_trap(f, pc);
    f.instruction(&W::End);
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::Call(context.physical_address_function));
    emit_store_value(f, kind, source, immediate);
}

fn emit_binary_i64(f: &mut Function, dst: RawReg, lhs: RawReg, rhs: RawReg, operation: W<'_>) {
    emit_reg(f, lhs);
    emit_reg(f, rhs);
    f.instruction(&operation);
    emit_set_reg(f, dst);
}

fn emit_binary_i32(f: &mut Function, dst: RawReg, lhs: RawReg, rhs: RawReg, operation: W<'_>) {
    emit_reg(f, lhs);
    f.instruction(&W::I32WrapI64);
    emit_reg(f, rhs);
    f.instruction(&W::I32WrapI64);
    emit_i32_result(f, dst, operation);
}

fn emit_binary_imm_i64(f: &mut Function, dst: RawReg, lhs: RawReg, imm: i32, operation: W<'_>) {
    emit_reg(f, lhs);
    f.instruction(&W::I64Const(imm as i64));
    f.instruction(&operation);
    emit_set_reg(f, dst);
}

fn emit_binary_imm_i32(f: &mut Function, dst: RawReg, lhs: RawReg, imm: i32, operation: W<'_>) {
    emit_reg(f, lhs);
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::I32Const(imm));
    emit_i32_result(f, dst, operation);
}

#[derive(Clone, Copy)]
enum CompareOp {
    Eq,
    Ne,
    LtU,
    LtS,
    LeU,
    LeS,
    GtU,
    GtS,
    GeU,
    GeS,
}

fn emit_compare_regs(
    context: &EmitContext<'_>,
    f: &mut Function,
    lhs: RawReg,
    rhs: RawReg,
    operation: CompareOp,
) {
    emit_reg(f, lhs);
    if !context.is_64_bit {
        f.instruction(&W::I32WrapI64);
    }
    emit_reg(f, rhs);
    if !context.is_64_bit {
        f.instruction(&W::I32WrapI64);
    }
    emit_compare_operation(context, f, operation);
}

fn emit_compare_imm(
    context: &EmitContext<'_>,
    f: &mut Function,
    lhs: RawReg,
    rhs: i32,
    operation: CompareOp,
) {
    emit_reg(f, lhs);
    if context.is_64_bit {
        f.instruction(&W::I64Const(rhs as i64));
    } else {
        f.instruction(&W::I32WrapI64);
        f.instruction(&W::I32Const(rhs));
    }
    emit_compare_operation(context, f, operation);
}

fn emit_compare_operation(context: &EmitContext<'_>, f: &mut Function, operation: CompareOp) {
    f.instruction(&match (context.is_64_bit, operation) {
        (true, CompareOp::Eq) => W::I64Eq,
        (true, CompareOp::Ne) => W::I64Ne,
        (true, CompareOp::LtU) => W::I64LtU,
        (true, CompareOp::LtS) => W::I64LtS,
        (true, CompareOp::LeU) => W::I64LeU,
        (true, CompareOp::LeS) => W::I64LeS,
        (true, CompareOp::GtU) => W::I64GtU,
        (true, CompareOp::GtS) => W::I64GtS,
        (true, CompareOp::GeU) => W::I64GeU,
        (true, CompareOp::GeS) => W::I64GeS,
        (false, CompareOp::Eq) => W::I32Eq,
        (false, CompareOp::Ne) => W::I32Ne,
        (false, CompareOp::LtU) => W::I32LtU,
        (false, CompareOp::LtS) => W::I32LtS,
        (false, CompareOp::LeU) => W::I32LeU,
        (false, CompareOp::LeS) => W::I32LeS,
        (false, CompareOp::GtU) => W::I32GtU,
        (false, CompareOp::GtS) => W::I32GtS,
        (false, CompareOp::GeU) => W::I32GeU,
        (false, CompareOp::GeS) => W::I32GeS,
    });
}

fn emit_compare_branch(
    context: &EmitContext<'_>,
    f: &mut Function,
    instruction: &ParsedInstruction,
    target: u32,
) -> Result<()> {
    f.instruction(&W::If(BlockType::Empty));
    emit_block_target(context, f, target)?;
    f.instruction(&W::Else);
    emit_block_target(context, f, instruction.next_offset.0)?;
    f.instruction(&W::End);
    f.instruction(&W::Unreachable);
    Ok(())
}

fn emit_indirect(context: &EmitContext<'_>, f: &mut Function, pc: u32, base: RawReg, offset: i32) {
    emit_reg(f, base);
    f.instruction(&W::I64Const(offset as i64));
    f.instruction(&W::I64Add);
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::LocalSet(LOCAL_ADDR));
    emit_indirect_from_local(context, f, pc, base != Reg::RA.raw() || offset != 0);
}

// Keep the caller's target capture before fused register writes, then tail-call
// shared validation/resolution. A regular call would retain a Wasm stack frame
// across every guest return or indirect jump.
fn emit_indirect_from_local(context: &EmitContext<'_>, f: &mut Function, pc: u32, meter: bool) {
    let current_pc = context.control_targets[context
        .control_targets
        .partition_point(|target| *target <= pc)
        - 1];
    let current_block = context.block_by_pc.get(current_pc).unwrap();
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(pc as i32));
    f.instruction(&W::I32Const(current_block as i32));
    f.instruction(&W::ReturnCall(if meter {
        context.indirect_function
    } else {
        context.return_function
    }));
}

fn emit_indirect_helper(context: &EmitContext<'_>, meter: bool) -> Function {
    // Parameters: target address, source instruction PC, original source block.
    // Specialize the backedge comparison for returns rather than passing a flag
    // and branching at every callsite.
    let mut f = Function::new([]);
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(RETURN_TO_HOST as i32));
    f.instruction(&W::I32Eq);
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::I32Const(STATUS_FINISHED));
    f.instruction(&W::Return);
    f.instruction(&W::End);
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(1));
    f.instruction(&W::I32And);
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Eqz);
    f.instruction(&W::I32Or);
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::LocalGet(1));
    f.instruction(&W::GlobalSet(GLOBAL_TRAP_PC));
    f.instruction(&W::I32Const(STATUS_TRAP));
    f.instruction(&W::Return);
    f.instruction(&W::End);
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(2));
    f.instruction(&W::I32DivU);
    f.instruction(&W::I32Const(1));
    f.instruction(&W::I32Sub);
    f.instruction(&W::LocalTee(LOCAL_ADDR));
    f.instruction(&W::I32Const(context.jump_table_len as i32));
    f.instruction(&W::I32GeU);
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::LocalGet(1));
    f.instruction(&W::GlobalSet(GLOBAL_TRAP_PC));
    f.instruction(&W::I32Const(STATUS_TRAP));
    f.instruction(&W::Return);
    f.instruction(&W::End);
    // The resolver takes the full jump-table index and switches on its low
    // bits. Only one table entry/function is needed per resolver group.
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(RESOLVERS_PER_FUNCTION.trailing_zeros() as i32));
    f.instruction(&W::I32ShrU);
    f.instruction(&W::I32Const(context.resolver_table_base as i32));
    f.instruction(&W::I32Add);
    f.instruction(&W::CallIndirect {
        type_index: TYPE_RESOLVER,
        table_index: 0,
    });
    f.instruction(&W::LocalTee(LOCAL_ADDR));
    f.instruction(&W::GlobalSet(GLOBAL_PC));
    // Resume at the resolved destination, not at the source block: a fused
    // load-and-jump may already have overwritten the register holding its target.
    // Compare against the original control-flow block, not an artificial split.
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::LocalGet(2));
    f.instruction(if meter { &W::I32LeU } else { &W::I32Eq });
    f.instruction(&W::If(BlockType::Empty));
    emit_gas_charge(&mut f, context.gas_function);
    f.instruction(&W::End);
    emit_dispatch_from_pc(&mut f);
    f.instruction(&W::End);
    f
}

fn written_register(kind: PvmInstruction) -> Option<RawReg> {
    use PvmInstruction::*;
    Some(match kind {
        load_imm(dst, _)
        | load_imm64(dst, _)
        | move_reg(dst, _)
        | load_u8(dst, _)
        | load_i8(dst, _)
        | load_u16(dst, _)
        | load_i16(dst, _)
        | load_u32(dst, _)
        | load_i32(dst, _)
        | load_u64(dst, _)
        | load_indirect_u8(dst, _, _)
        | load_indirect_i8(dst, _, _)
        | load_indirect_u16(dst, _, _)
        | load_indirect_i16(dst, _, _)
        | load_indirect_u32(dst, _, _)
        | load_indirect_i32(dst, _, _)
        | load_indirect_u64(dst, _, _)
        | add_32(dst, _, _)
        | add_64(dst, _, _)
        | sub_32(dst, _, _)
        | sub_64(dst, _, _)
        | and(dst, _, _)
        | xor(dst, _, _)
        | or(dst, _, _)
        | and_inverted(dst, _, _)
        | or_inverted(dst, _, _)
        | xnor(dst, _, _)
        | mul_32(dst, _, _)
        | mul_64(dst, _, _)
        | mul_upper_unsigned_unsigned(dst, _, _)
        | mul_upper_signed_signed(dst, _, _)
        | mul_upper_signed_unsigned(dst, _, _)
        | add_imm_32(dst, _, _)
        | add_imm_64(dst, _, _)
        | and_imm(dst, _, _)
        | xor_imm(dst, _, _)
        | or_imm(dst, _, _)
        | mul_imm_32(dst, _, _)
        | mul_imm_64(dst, _, _)
        | negate_and_add_imm_32(dst, _, _)
        | negate_and_add_imm_64(dst, _, _)
        | set_less_than_unsigned(dst, _, _)
        | set_less_than_signed(dst, _, _)
        | set_less_than_unsigned_imm(dst, _, _)
        | set_less_than_signed_imm(dst, _, _)
        | set_greater_than_unsigned_imm(dst, _, _)
        | set_greater_than_signed_imm(dst, _, _)
        | shift_logical_left_32(dst, _, _)
        | shift_logical_left_64(dst, _, _)
        | shift_logical_right_32(dst, _, _)
        | shift_logical_right_64(dst, _, _)
        | shift_arithmetic_right_32(dst, _, _)
        | shift_arithmetic_right_64(dst, _, _)
        | shift_logical_left_imm_32(dst, _, _)
        | shift_logical_left_imm_64(dst, _, _)
        | shift_logical_right_imm_32(dst, _, _)
        | shift_logical_right_imm_64(dst, _, _)
        | shift_arithmetic_right_imm_32(dst, _, _)
        | shift_arithmetic_right_imm_64(dst, _, _)
        | shift_logical_right_imm_alt_32(dst, _, _)
        | shift_logical_right_imm_alt_64(dst, _, _)
        | shift_arithmetic_right_imm_alt_32(dst, _, _)
        | shift_arithmetic_right_imm_alt_64(dst, _, _)
        | shift_logical_left_imm_alt_32(dst, _, _)
        | shift_logical_left_imm_alt_64(dst, _, _)
        | rotate_left_32(dst, _, _)
        | rotate_left_64(dst, _, _)
        | rotate_right_32(dst, _, _)
        | rotate_right_64(dst, _, _)
        | rotate_right_imm_32(dst, _, _)
        | rotate_right_imm_64(dst, _, _)
        | rotate_right_imm_alt_32(dst, _, _)
        | rotate_right_imm_alt_64(dst, _, _)
        | div_unsigned_32(dst, _, _)
        | div_unsigned_64(dst, _, _)
        | div_signed_32(dst, _, _)
        | div_signed_64(dst, _, _)
        | rem_unsigned_32(dst, _, _)
        | rem_unsigned_64(dst, _, _)
        | rem_signed_32(dst, _, _)
        | rem_signed_64(dst, _, _)
        | count_leading_zero_bits_32(dst, _)
        | count_leading_zero_bits_64(dst, _)
        | count_trailing_zero_bits_32(dst, _)
        | count_trailing_zero_bits_64(dst, _)
        | count_set_bits_32(dst, _)
        | count_set_bits_64(dst, _)
        | sign_extend_8(dst, _)
        | sign_extend_16(dst, _)
        | zero_extend_16(dst, _)
        | reverse_byte(dst, _)
        | maximum(dst, _, _)
        | maximum_unsigned(dst, _, _)
        | minimum(dst, _, _)
        | minimum_unsigned(dst, _, _)
        | cmov_if_zero(dst, _, _)
        | cmov_if_not_zero(dst, _, _)
        | cmov_if_zero_imm(dst, _, _)
        | cmov_if_not_zero_imm(dst, _, _)
        | sbrk(dst, _) => dst,
        _ => return None,
    })
}

fn normalize_32(f: &mut Function, reg: RawReg) {
    emit_reg(f, reg);
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::I64ExtendI32U);
    emit_set_reg(f, reg);
}

fn emit_instruction(
    context: &EmitContext<'_>,
    f: &mut Function,
    instruction: &ParsedInstruction,
) -> Result<bool> {
    use PvmInstruction::*;
    let pc = instruction.offset.0;
    match instruction.kind {
        trap | invalid => {
            emit_trap(f, pc);
            return Ok(true);
        }
        fallthrough | unlikely => {}
        memset => emit_memset(context, f, pc),
        jump(target) => {
            emit_return_target(context, f, target)?;
            return Ok(true);
        }
        jump_indirect(base, offset) => {
            emit_indirect(context, f, pc, base, offset);
            return Ok(true);
        }
        load_imm_and_jump(dst, value, target) => {
            f.instruction(&W::I64Const(if context.is_64_bit {
                value as i64
            } else {
                value as u32 as i64
            }));
            emit_set_reg(f, dst);
            emit_return_target(context, f, target)?;
            return Ok(true);
        }
        load_imm_and_jump_indirect(dst, base, value, offset) => {
            // The target address uses the old base value when dst aliases base.
            emit_reg(f, base);
            f.instruction(&W::I64Const(offset as i64));
            f.instruction(&W::I64Add);
            f.instruction(&W::I32WrapI64);
            f.instruction(&W::LocalSet(LOCAL_ADDR));
            f.instruction(&W::I64Const(if context.is_64_bit {
                value as i64
            } else {
                value as u32 as i64
            }));
            emit_set_reg(f, dst);
            emit_indirect_from_local(context, f, pc, true);
            return Ok(true);
        }
        ecalli(index) => {
            f.instruction(&W::I32Const(index));
            f.instruction(&W::GlobalSet(GLOBAL_ECALL));
            let next = context
                .block_by_pc
                .get(instruction.next_offset.0)
                .ok_or_else(|| anyhow!("ecall continuation is not a block"))?;
            f.instruction(&W::I32Const(next as i32));
            f.instruction(&W::GlobalSet(GLOBAL_PC));
            f.instruction(&W::I32Const(STATUS_ECALL));
            f.instruction(&W::Return);
            return Ok(true);
        }
        load_imm(dst, value) => {
            f.instruction(&W::I64Const(value as i64));
            emit_set_reg(f, dst);
        }
        load_imm64(dst, value) => {
            f.instruction(&W::I64Const(value as i64));
            emit_set_reg(f, dst);
        }
        move_reg(dst, src) => {
            emit_reg(f, src);
            emit_set_reg(f, dst);
        }
        load_u8(dst, offset) => emit_load(context, f, pc, dst, None, offset, LoadKind::U8),
        load_i8(dst, offset) => emit_load(context, f, pc, dst, None, offset, LoadKind::I8),
        load_u16(dst, offset) => emit_load(context, f, pc, dst, None, offset, LoadKind::U16),
        load_i16(dst, offset) => emit_load(context, f, pc, dst, None, offset, LoadKind::I16),
        load_u32(dst, offset) => emit_load(context, f, pc, dst, None, offset, LoadKind::U32),
        load_i32(dst, offset) => emit_load(context, f, pc, dst, None, offset, LoadKind::I32),
        load_u64(dst, offset) => emit_load(context, f, pc, dst, None, offset, LoadKind::U64),
        load_indirect_u8(dst, base, offset) => {
            emit_load(context, f, pc, dst, Some(base), offset, LoadKind::U8)
        }
        load_indirect_i8(dst, base, offset) => {
            emit_load(context, f, pc, dst, Some(base), offset, LoadKind::I8)
        }
        load_indirect_u16(dst, base, offset) => {
            emit_load(context, f, pc, dst, Some(base), offset, LoadKind::U16)
        }
        load_indirect_i16(dst, base, offset) => {
            emit_load(context, f, pc, dst, Some(base), offset, LoadKind::I16)
        }
        load_indirect_u32(dst, base, offset) => {
            emit_load(context, f, pc, dst, Some(base), offset, LoadKind::U32)
        }
        load_indirect_i32(dst, base, offset) => {
            emit_load(context, f, pc, dst, Some(base), offset, LoadKind::I32)
        }
        load_indirect_u64(dst, base, offset) => {
            emit_load(context, f, pc, dst, Some(base), offset, LoadKind::U64)
        }
        store_u8(src, offset) => {
            emit_store(context, f, pc, Some(src), None, offset, 0, StoreKind::U8)
        }
        store_u16(src, offset) => {
            emit_store(context, f, pc, Some(src), None, offset, 0, StoreKind::U16)
        }
        store_u32(src, offset) => {
            emit_store(context, f, pc, Some(src), None, offset, 0, StoreKind::U32)
        }
        store_u64(src, offset) => {
            emit_store(context, f, pc, Some(src), None, offset, 0, StoreKind::U64)
        }
        store_indirect_u8(src, base, offset) => emit_store(
            context,
            f,
            pc,
            Some(src),
            Some(base),
            offset,
            0,
            StoreKind::U8,
        ),
        store_indirect_u16(src, base, offset) => emit_store(
            context,
            f,
            pc,
            Some(src),
            Some(base),
            offset,
            0,
            StoreKind::U16,
        ),
        store_indirect_u32(src, base, offset) => emit_store(
            context,
            f,
            pc,
            Some(src),
            Some(base),
            offset,
            0,
            StoreKind::U32,
        ),
        store_indirect_u64(src, base, offset) => emit_store(
            context,
            f,
            pc,
            Some(src),
            Some(base),
            offset,
            0,
            StoreKind::U64,
        ),
        store_imm_u8(offset, value) => {
            emit_store(context, f, pc, None, None, offset, value, StoreKind::U8)
        }
        store_imm_u16(offset, value) => {
            emit_store(context, f, pc, None, None, offset, value, StoreKind::U16)
        }
        store_imm_u32(offset, value) => {
            emit_store(context, f, pc, None, None, offset, value, StoreKind::U32)
        }
        store_imm_u64(offset, value) => {
            emit_store(context, f, pc, None, None, offset, value, StoreKind::U64)
        }
        store_imm_indirect_u8(base, offset, value) => emit_store(
            context,
            f,
            pc,
            None,
            Some(base),
            offset,
            value,
            StoreKind::U8,
        ),
        store_imm_indirect_u16(base, offset, value) => emit_store(
            context,
            f,
            pc,
            None,
            Some(base),
            offset,
            value,
            StoreKind::U16,
        ),
        store_imm_indirect_u32(base, offset, value) => emit_store(
            context,
            f,
            pc,
            None,
            Some(base),
            offset,
            value,
            StoreKind::U32,
        ),
        store_imm_indirect_u64(base, offset, value) => emit_store(
            context,
            f,
            pc,
            None,
            Some(base),
            offset,
            value,
            StoreKind::U64,
        ),
        add_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32Add),
        add_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Add),
        sub_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32Sub),
        sub_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Sub),
        and(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64And),
        xor(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Xor),
        or(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Or),
        and_inverted(dst, a, b) => {
            emit_reg(f, a);
            emit_reg(f, b);
            f.instruction(&W::I64Const(-1));
            f.instruction(&W::I64Xor);
            f.instruction(&W::I64And);
            emit_set_reg(f, dst);
        }
        or_inverted(dst, a, b) => {
            emit_reg(f, a);
            emit_reg(f, b);
            f.instruction(&W::I64Const(-1));
            f.instruction(&W::I64Xor);
            f.instruction(&W::I64Or);
            emit_set_reg(f, dst);
        }
        xnor(dst, a, b) => {
            emit_reg(f, a);
            emit_reg(f, b);
            f.instruction(&W::I64Xor);
            f.instruction(&W::I64Const(-1));
            f.instruction(&W::I64Xor);
            emit_set_reg(f, dst);
        }
        mul_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32Mul),
        mul_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Mul),
        mul_upper_unsigned_unsigned(dst, a, b) => {
            emit_mul_high(context, f, dst, a, b, false, false);
        }
        mul_upper_signed_signed(dst, a, b) => emit_mul_high(context, f, dst, a, b, true, true),
        mul_upper_signed_unsigned(dst, a, b) => emit_mul_high(context, f, dst, a, b, true, false),
        add_imm_32(dst, a, imm) => emit_binary_imm_i32(f, dst, a, imm, W::I32Add),
        add_imm_64(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64Add),
        and_imm(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64And),
        xor_imm(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64Xor),
        or_imm(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64Or),
        mul_imm_32(dst, a, imm) => emit_binary_imm_i32(f, dst, a, imm, W::I32Mul),
        mul_imm_64(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64Mul),
        negate_and_add_imm_32(dst, a, imm) => {
            f.instruction(&W::I32Const(imm));
            emit_reg(f, a);
            f.instruction(&W::I32WrapI64);
            emit_i32_result(f, dst, W::I32Sub);
        }
        negate_and_add_imm_64(dst, a, imm) => {
            f.instruction(&W::I64Const(imm as i64));
            emit_reg(f, a);
            f.instruction(&W::I64Sub);
            emit_set_reg(f, dst);
        }
        set_less_than_unsigned(dst, a, b) => emit_comparison(context, f, dst, a, b, CompareOp::LtU),
        set_less_than_signed(dst, a, b) => emit_comparison(context, f, dst, a, b, CompareOp::LtS),
        set_less_than_unsigned_imm(dst, a, imm) => {
            emit_comparison_imm(context, f, dst, a, imm, CompareOp::LtU)
        }
        set_less_than_signed_imm(dst, a, imm) => {
            emit_comparison_imm(context, f, dst, a, imm, CompareOp::LtS)
        }
        set_greater_than_unsigned_imm(dst, a, imm) => {
            emit_comparison_imm(context, f, dst, a, imm, CompareOp::GtU)
        }
        set_greater_than_signed_imm(dst, a, imm) => {
            emit_comparison_imm(context, f, dst, a, imm, CompareOp::GtS)
        }
        shift_logical_left_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32Shl),
        shift_logical_left_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Shl),
        shift_logical_right_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32ShrU),
        shift_logical_right_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64ShrU),
        shift_arithmetic_right_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32ShrS),
        shift_arithmetic_right_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64ShrS),
        shift_logical_left_imm_32(dst, a, imm) => emit_binary_imm_i32(f, dst, a, imm, W::I32Shl),
        shift_logical_left_imm_64(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64Shl),
        shift_logical_right_imm_32(dst, a, imm) => emit_binary_imm_i32(f, dst, a, imm, W::I32ShrU),
        shift_logical_right_imm_64(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64ShrU),
        shift_arithmetic_right_imm_32(dst, a, imm) => {
            emit_binary_imm_i32(f, dst, a, imm, W::I32ShrS)
        }
        shift_arithmetic_right_imm_64(dst, a, imm) => {
            emit_binary_imm_i64(f, dst, a, imm, W::I64ShrS)
        }
        shift_logical_right_imm_alt_32(dst, a, imm) => {
            emit_alt_shift_i32(f, dst, a, imm, W::I32ShrU)
        }
        shift_logical_right_imm_alt_64(dst, a, imm) => {
            emit_alt_shift_i64(f, dst, a, imm, W::I64ShrU)
        }
        shift_arithmetic_right_imm_alt_32(dst, a, imm) => {
            emit_alt_shift_i32(f, dst, a, imm, W::I32ShrS)
        }
        shift_arithmetic_right_imm_alt_64(dst, a, imm) => {
            emit_alt_shift_i64(f, dst, a, imm, W::I64ShrS)
        }
        shift_logical_left_imm_alt_32(dst, a, imm) => emit_alt_shift_i32(f, dst, a, imm, W::I32Shl),
        shift_logical_left_imm_alt_64(dst, a, imm) => emit_alt_shift_i64(f, dst, a, imm, W::I64Shl),
        rotate_left_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32Rotl),
        rotate_left_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Rotl),
        rotate_right_32(dst, a, b) => emit_binary_i32(f, dst, a, b, W::I32Rotr),
        rotate_right_64(dst, a, b) => emit_binary_i64(f, dst, a, b, W::I64Rotr),
        rotate_right_imm_32(dst, a, imm) => emit_binary_imm_i32(f, dst, a, imm, W::I32Rotr),
        rotate_right_imm_64(dst, a, imm) => emit_binary_imm_i64(f, dst, a, imm, W::I64Rotr),
        rotate_right_imm_alt_32(dst, a, imm) => emit_alt_shift_i32(f, dst, a, imm, W::I32Rotr),
        rotate_right_imm_alt_64(dst, a, imm) => emit_alt_shift_i64(f, dst, a, imm, W::I64Rotr),
        div_unsigned_32(dst, a, b) => emit_divrem(f, dst, a, b, false, false, true),
        div_unsigned_64(dst, a, b) => emit_divrem(f, dst, a, b, false, false, false),
        div_signed_32(dst, a, b) => emit_divrem(f, dst, a, b, true, false, true),
        div_signed_64(dst, a, b) => emit_divrem(f, dst, a, b, true, false, false),
        rem_unsigned_32(dst, a, b) => emit_divrem(f, dst, a, b, false, true, true),
        rem_unsigned_64(dst, a, b) => emit_divrem(f, dst, a, b, false, true, false),
        rem_signed_32(dst, a, b) => emit_divrem(f, dst, a, b, true, true, true),
        rem_signed_64(dst, a, b) => emit_divrem(f, dst, a, b, true, true, false),
        count_leading_zero_bits_32(dst, src) => emit_unary_i32(f, dst, src, W::I32Clz),
        count_leading_zero_bits_64(dst, src) => emit_unary_i64(f, dst, src, W::I64Clz),
        count_trailing_zero_bits_32(dst, src) => emit_unary_i32(f, dst, src, W::I32Ctz),
        count_trailing_zero_bits_64(dst, src) => emit_unary_i64(f, dst, src, W::I64Ctz),
        count_set_bits_32(dst, src) => emit_unary_i32(f, dst, src, W::I32Popcnt),
        count_set_bits_64(dst, src) => emit_unary_i64(f, dst, src, W::I64Popcnt),
        sign_extend_8(dst, src) => {
            emit_reg(f, src);
            f.instruction(&W::I64Extend8S);
            emit_set_reg(f, dst);
        }
        sign_extend_16(dst, src) => {
            emit_reg(f, src);
            f.instruction(&W::I64Extend16S);
            emit_set_reg(f, dst);
        }
        zero_extend_16(dst, src) => {
            emit_reg(f, src);
            f.instruction(&W::I64Const(0xffff));
            f.instruction(&W::I64And);
            emit_set_reg(f, dst);
        }
        reverse_byte(dst, src) => {
            emit_reg(f, src);
            f.instruction(&W::Call(if context.is_64_bit {
                context.bswap64_function
            } else {
                context.bswap32_function
            }));
            emit_set_reg(f, dst);
        }
        maximum(dst, a, b) => emit_minmax(f, dst, a, b, true, false, context.is_64_bit),
        maximum_unsigned(dst, a, b) => emit_minmax(f, dst, a, b, true, true, context.is_64_bit),
        minimum(dst, a, b) => emit_minmax(f, dst, a, b, false, false, context.is_64_bit),
        minimum_unsigned(dst, a, b) => emit_minmax(f, dst, a, b, false, true, context.is_64_bit),
        cmov_if_zero(dst, src, condition) => emit_cmov(f, dst, src, condition, true),
        cmov_if_not_zero(dst, src, condition) => emit_cmov(f, dst, src, condition, false),
        cmov_if_zero_imm(dst, condition, value) => emit_cmov_imm(f, dst, condition, value, true),
        cmov_if_not_zero_imm(dst, condition, value) => {
            emit_cmov_imm(f, dst, condition, value, false)
        }
        sbrk(dst, size) => emit_sbrk(context, f, dst, size),
        branch_eq(a, b, target) => {
            emit_compare_regs(context, f, a, b, CompareOp::Eq);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_not_eq(a, b, target) => {
            emit_compare_regs(context, f, a, b, CompareOp::Ne);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_less_unsigned(a, b, target) => {
            emit_compare_regs(context, f, a, b, CompareOp::LtU);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_less_signed(a, b, target) => {
            emit_compare_regs(context, f, a, b, CompareOp::LtS);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_greater_or_equal_unsigned(a, b, target) => {
            emit_compare_regs(context, f, a, b, CompareOp::GeU);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_greater_or_equal_signed(a, b, target) => {
            emit_compare_regs(context, f, a, b, CompareOp::GeS);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_eq_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::Eq);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_not_eq_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::Ne);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_less_unsigned_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::LtU);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_less_signed_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::LtS);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_greater_or_equal_unsigned_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::GeU);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_greater_or_equal_signed_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::GeS);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_less_or_equal_unsigned_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::LeU);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_less_or_equal_signed_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::LeS);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_greater_unsigned_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::GtU);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
        branch_greater_signed_imm(a, imm, target) => {
            emit_compare_imm(context, f, a, imm, CompareOp::GtS);
            emit_compare_branch(context, f, instruction, target)?;
            return Ok(true);
        }
    }
    if !context.is_64_bit {
        if instruction.kind == PvmInstruction::memset {
            normalize_32(f, Reg::A0.raw());
            normalize_32(f, Reg::A2.raw());
        } else if let Some(reg) = written_register(instruction.kind) {
            normalize_32(f, reg);
        }
    }
    Ok(false)
}

fn emit_memset(context: &EmitContext<'_>, f: &mut Function, pc: u32) {
    emit_reg(f, Reg::A0.raw());
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::LocalSet(LOCAL_ADDR));
    emit_reg(f, Reg::A2.raw());
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::LocalSet(LOCAL_PHYS));
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(context.layout.stack_low as i32));
    f.instruction(&W::I32GeU);
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(context.layout.stack_low as i32));
    f.instruction(&W::I32Sub);
    emit_reg(f, Reg::A1.raw());
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::LocalGet(LOCAL_PHYS));
    f.instruction(&W::MemoryFill(2));
    f.instruction(&W::Else);
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(context.layout.rw_address as i32));
    f.instruction(&W::I32GeU);
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::LocalGet(LOCAL_ADDR));
    f.instruction(&W::I32Const(context.layout.rw_address as i32));
    f.instruction(&W::I32Sub);
    emit_reg(f, Reg::A1.raw());
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::LocalGet(LOCAL_PHYS));
    f.instruction(&W::MemoryFill(1));
    f.instruction(&W::Else);
    emit_trap(f, pc);
    f.instruction(&W::End);
    f.instruction(&W::End);
    emit_reg(f, Reg::A0.raw());
    emit_reg(f, Reg::A2.raw());
    f.instruction(&W::I64Add);
    emit_set_reg(f, Reg::A0.raw());
    f.instruction(&W::I64Const(0));
    emit_set_reg(f, Reg::A2.raw());
}

fn emit_mul_high(
    context: &EmitContext<'_>,
    f: &mut Function,
    dst: RawReg,
    a: RawReg,
    b: RawReg,
    signed_a: bool,
    signed_b: bool,
) {
    if !context.is_64_bit {
        emit_reg(f, a);
        f.instruction(&W::I32WrapI64);
        f.instruction(&if signed_a {
            W::I64ExtendI32S
        } else {
            W::I64ExtendI32U
        });
        emit_reg(f, b);
        f.instruction(&W::I32WrapI64);
        f.instruction(&if signed_b {
            W::I64ExtendI32S
        } else {
            W::I64ExtendI32U
        });
        f.instruction(&W::I64Mul);
        f.instruction(&W::I64Const(32));
        f.instruction(&W::I64ShrU);
        emit_set_reg(f, dst);
        return;
    }
    emit_reg(f, a);
    f.instruction(&W::LocalSet(LOCAL_I64_0));
    emit_reg(f, b);
    f.instruction(&W::LocalSet(LOCAL_I64_1));
    f.instruction(&W::LocalGet(LOCAL_I64_0));
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    f.instruction(&W::Call(context.mulhu_function));
    if signed_a {
        f.instruction(&W::LocalGet(LOCAL_I64_0));
        f.instruction(&W::I64Const(0));
        f.instruction(&W::I64LtS);
        f.instruction(&W::If(BlockType::Result(ValType::I64)));
        f.instruction(&W::LocalGet(LOCAL_I64_1));
        f.instruction(&W::Else);
        f.instruction(&W::I64Const(0));
        f.instruction(&W::End);
        f.instruction(&W::I64Sub);
    }
    if signed_b {
        f.instruction(&W::LocalGet(LOCAL_I64_1));
        f.instruction(&W::I64Const(0));
        f.instruction(&W::I64LtS);
        f.instruction(&W::If(BlockType::Result(ValType::I64)));
        f.instruction(&W::LocalGet(LOCAL_I64_0));
        f.instruction(&W::Else);
        f.instruction(&W::I64Const(0));
        f.instruction(&W::End);
        f.instruction(&W::I64Sub);
    }
    emit_set_reg(f, dst);
}

fn emit_comparison(
    context: &EmitContext<'_>,
    f: &mut Function,
    dst: RawReg,
    a: RawReg,
    b: RawReg,
    operation: CompareOp,
) {
    emit_compare_regs(context, f, a, b, operation);
    f.instruction(&W::I64ExtendI32U);
    emit_set_reg(f, dst);
}

fn emit_comparison_imm(
    context: &EmitContext<'_>,
    f: &mut Function,
    dst: RawReg,
    a: RawReg,
    imm: i32,
    operation: CompareOp,
) {
    emit_compare_imm(context, f, a, imm, operation);
    f.instruction(&W::I64ExtendI32U);
    emit_set_reg(f, dst);
}
fn emit_alt_shift_i32(f: &mut Function, dst: RawReg, a: RawReg, imm: i32, operation: W<'_>) {
    f.instruction(&W::I32Const(imm));
    emit_reg(f, a);
    f.instruction(&W::I32WrapI64);
    emit_i32_result(f, dst, operation);
}
fn emit_alt_shift_i64(f: &mut Function, dst: RawReg, a: RawReg, imm: i32, operation: W<'_>) {
    f.instruction(&W::I64Const(imm as i64));
    emit_reg(f, a);
    f.instruction(&operation);
    emit_set_reg(f, dst);
}
fn emit_unary_i32(f: &mut Function, dst: RawReg, src: RawReg, operation: W<'_>) {
    emit_reg(f, src);
    f.instruction(&W::I32WrapI64);
    emit_i32_result(f, dst, operation);
}
fn emit_unary_i64(f: &mut Function, dst: RawReg, src: RawReg, operation: W<'_>) {
    emit_reg(f, src);
    f.instruction(&operation);
    emit_set_reg(f, dst);
}

fn emit_divrem(
    f: &mut Function,
    dst: RawReg,
    a: RawReg,
    b: RawReg,
    signed: bool,
    remainder: bool,
    bits32: bool,
) {
    emit_reg(f, a);
    f.instruction(&W::LocalSet(LOCAL_I64_0));
    emit_reg(f, b);
    f.instruction(&W::LocalSet(LOCAL_I64_1));
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    if bits32 {
        f.instruction(&W::I32WrapI64);
        f.instruction(&W::I32Eqz);
    } else {
        f.instruction(&W::I64Eqz);
    }
    f.instruction(&W::If(BlockType::Result(ValType::I64)));
    if remainder {
        f.instruction(&W::LocalGet(LOCAL_I64_0));
        if bits32 {
            f.instruction(&W::I32WrapI64);
            f.instruction(&W::I64ExtendI32S);
        }
    } else {
        f.instruction(&W::I64Const(-1));
    }
    f.instruction(&W::Else);
    if signed {
        f.instruction(&W::LocalGet(LOCAL_I64_0));
        if bits32 {
            f.instruction(&W::I32WrapI64);
            f.instruction(&W::I32Const(i32::MIN));
            f.instruction(&W::I32Eq);
            f.instruction(&W::LocalGet(LOCAL_I64_1));
            f.instruction(&W::I32WrapI64);
            f.instruction(&W::I32Const(-1));
            f.instruction(&W::I32Eq);
        } else {
            f.instruction(&W::I64Const(i64::MIN));
            f.instruction(&W::I64Eq);
            f.instruction(&W::LocalGet(LOCAL_I64_1));
            f.instruction(&W::I64Const(-1));
            f.instruction(&W::I64Eq);
        }
        f.instruction(&W::I32And);
        f.instruction(&W::If(BlockType::Result(ValType::I64)));
        f.instruction(&W::I64Const(if remainder {
            0
        } else if bits32 {
            i32::MIN as i64
        } else {
            i64::MIN
        }));
        f.instruction(&W::Else);
        emit_divrem_operation(f, signed, remainder, bits32);
        f.instruction(&W::End);
    } else {
        emit_divrem_operation(f, signed, remainder, bits32);
    }
    f.instruction(&W::End);
    emit_set_reg(f, dst);
}

fn emit_divrem_operation(f: &mut Function, signed: bool, remainder: bool, bits32: bool) {
    f.instruction(&W::LocalGet(LOCAL_I64_0));
    if bits32 {
        f.instruction(&W::I32WrapI64);
    }
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    if bits32 {
        f.instruction(&W::I32WrapI64);
        f.instruction(&match (signed, remainder) {
            (true, true) => W::I32RemS,
            (true, false) => W::I32DivS,
            (false, true) => W::I32RemU,
            (false, false) => W::I32DivU,
        });
        f.instruction(&W::I64ExtendI32S);
    } else {
        f.instruction(&match (signed, remainder) {
            (true, true) => W::I64RemS,
            (true, false) => W::I64DivS,
            (false, true) => W::I64RemU,
            (false, false) => W::I64DivU,
        });
    }
}

fn emit_minmax(
    f: &mut Function,
    dst: RawReg,
    a: RawReg,
    b: RawReg,
    maximum: bool,
    unsigned: bool,
    bits64: bool,
) {
    emit_reg(f, a);
    f.instruction(&W::LocalSet(LOCAL_I64_0));
    emit_reg(f, b);
    f.instruction(&W::LocalSet(LOCAL_I64_1));
    f.instruction(&W::LocalGet(LOCAL_I64_0));
    if !bits64 {
        f.instruction(&W::I32WrapI64);
    }
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    if !bits64 {
        f.instruction(&W::I32WrapI64);
    }
    f.instruction(&match (maximum, unsigned, bits64) {
        (true, true, true) => W::I64GtU,
        (true, false, true) => W::I64GtS,
        (false, true, true) => W::I64LtU,
        (false, false, true) => W::I64LtS,
        (true, true, false) => W::I32GtU,
        (true, false, false) => W::I32GtS,
        (false, true, false) => W::I32LtU,
        (false, false, false) => W::I32LtS,
    });
    f.instruction(&W::If(BlockType::Result(ValType::I64)));
    f.instruction(&W::LocalGet(LOCAL_I64_0));
    f.instruction(&W::Else);
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    f.instruction(&W::End);
    if !bits64 {
        f.instruction(&W::I32WrapI64);
        f.instruction(&W::I64ExtendI32S);
    }
    emit_set_reg(f, dst);
}

fn emit_cmov(f: &mut Function, dst: RawReg, src: RawReg, condition: RawReg, zero: bool) {
    emit_reg(f, condition);
    f.instruction(&W::I64Eqz);
    if !zero {
        f.instruction(&W::I32Eqz);
    }
    f.instruction(&W::If(BlockType::Empty));
    emit_reg(f, src);
    emit_set_reg(f, dst);
    f.instruction(&W::End);
}
fn emit_cmov_imm(f: &mut Function, dst: RawReg, condition: RawReg, value: i32, zero: bool) {
    emit_reg(f, condition);
    f.instruction(&W::I64Eqz);
    if !zero {
        f.instruction(&W::I32Eqz);
    }
    f.instruction(&W::If(BlockType::Empty));
    f.instruction(&W::I64Const(value as i64));
    emit_set_reg(f, dst);
    f.instruction(&W::End);
}

fn emit_sbrk(context: &EmitContext<'_>, f: &mut Function, dst: RawReg, size: RawReg) {
    emit_reg(f, size);
    f.instruction(&W::LocalSet(LOCAL_I64_0));
    if context.is_64_bit {
        f.instruction(&W::LocalGet(LOCAL_I64_0));
        f.instruction(&W::I64Const(u32::MAX as i64));
        f.instruction(&W::I64GtU);
        f.instruction(&W::If(BlockType::Empty));
        f.instruction(&W::I64Const(0));
        f.instruction(&W::LocalSet(LOCAL_I64_0));
        f.instruction(&W::End);
    }
    f.instruction(&W::GlobalGet(GLOBAL_HEAP_SIZE));
    f.instruction(&W::LocalGet(LOCAL_I64_0));
    f.instruction(&W::I64Add);
    f.instruction(&W::LocalTee(LOCAL_I64_1));
    f.instruction(&W::I64Const(context.layout.heap_limit as i64));
    f.instruction(&W::I64LeU);
    f.instruction(&W::If(BlockType::Result(ValType::I64)));
    f.instruction(&W::I64Const(
        context
            .layout
            .heap_base
            .wrapping_sub(context.layout.rw_address) as i64,
    ));
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    f.instruction(&W::I64Add);
    f.instruction(&W::I64Const((PAGE_SIZE - 1) as i64));
    f.instruction(&W::I64Add);
    f.instruction(&W::I64Const(PAGE_SIZE.trailing_zeros() as i64));
    f.instruction(&W::I64ShrU);
    f.instruction(&W::I32WrapI64);
    f.instruction(&W::MemorySize(0));
    f.instruction(&W::I32Const((context.layout.rw_phys / PAGE_SIZE) as i32));
    f.instruction(&W::I32Sub);
    f.instruction(&W::I32Sub);
    f.instruction(&W::MemoryGrow(0));
    f.instruction(&W::I32Const(-1));
    f.instruction(&W::I32Ne);
    f.instruction(&W::If(BlockType::Result(ValType::I64)));
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    f.instruction(&W::GlobalSet(GLOBAL_HEAP_SIZE));
    f.instruction(&W::I64Const(context.layout.heap_base as i64));
    f.instruction(&W::LocalGet(LOCAL_I64_1));
    f.instruction(&W::I64Add);
    f.instruction(&W::Else);
    f.instruction(&W::I64Const(0));
    f.instruction(&W::End);
    f.instruction(&W::Else);
    f.instruction(&W::I64Const(0));
    f.instruction(&W::End);
    emit_set_reg(f, dst);
}

#[cfg(test)]
mod tests {
    use super::{
        translate, translate_partitioned, translate_with_part_limit, BLOCKS_PER_FUNCTION,
        CODE_PART_SECTION, INSTRUCTIONS_PER_BLOCK, STATUS_ECALL, STATUS_FINISHED,
        STATUS_OUT_OF_GAS, STATUS_TRAP,
    };
    use polkavm::program::{assemble, InstructionSetKind};
    use polkavm::{
        BackendKind, Config, Engine, InterruptKind, Module, ModuleConfig, ProgramBlob, Reg,
        RETURN_TO_HOST,
    };
    use polkavm_common::{program::asm, writer::ProgramBlobBuilder};
    use wasmi::{Engine as WasmEngine, Instance, Linker, Module as WasmModule, Store, Val};

    fn interpreter_registers(program: &[u8]) -> [u64; 13] {
        let blob = ProgramBlob::parse(program.into()).expect("parse differential fixture");
        let entry = blob
            .exports()
            .find(|export| export.symbol() == "main")
            .expect("main export")
            .program_counter();
        let mut config = Config::new();
        config.set_backend(Some(BackendKind::Interpreter));
        config.set_sandboxing_enabled(true);
        let engine = Engine::new(&config).expect("create interpreter");
        let module =
            Module::from_blob(&engine, &ModuleConfig::new(), blob).expect("compile fixture");
        let mut instance = module.instantiate().expect("instantiate fixture");
        instance.set_reg(Reg::SP, module.default_sp());
        instance.set_reg(Reg::RA, RETURN_TO_HOST);
        instance.set_gas(i64::MAX);
        instance.set_next_program_counter(entry);
        assert_eq!(
            instance.run().expect("run interpreter"),
            InterruptKind::Finished
        );
        Reg::ALL.map(|reg| instance.reg(reg))
    }

    fn translated_instance(program: &[u8]) -> (Store<()>, Instance) {
        // Exercise all existing group-boundary regressions across module boundaries.
        let wasm =
            translate_with_part_limit(program, Some(1)).expect("translate differential fixture");
        instance_from_wasm(&wasm)
    }

    fn code_parts(wasm: &[u8]) -> impl Iterator<Item = &[u8]> {
        wasmparser::Parser::new(0)
            .parse_all(wasm)
            .filter_map(|payload| match payload.expect("parse translated fixture") {
                wasmparser::Payload::CustomSection(section)
                    if section.name() == CODE_PART_SECTION =>
                {
                    Some(section.data())
                }
                _ => None,
            })
    }

    fn instance_from_wasm(wasm: &[u8]) -> (Store<()>, Instance) {
        let engine = WasmEngine::default();
        let module = WasmModule::new(&engine, wasm).expect("compile translated fixture");
        let mut store = Store::new(&engine, ());
        let mut linker = Linker::new(&engine);
        let instance = linker
            .instantiate(&mut store, &module)
            .expect("instantiate translated fixture")
            .start(&mut store)
            .expect("start translated fixture");
        linker
            .instance(&mut store, "pvm", instance)
            .expect("link translated shared state");
        for bytes in code_parts(wasm) {
            let part = WasmModule::new(&engine, bytes).expect("compile translated code part");
            // Wasmi retains instances in the Store, and the shared table retains
            // their functions. No entrypoint runs until every segment is installed.
            linker
                .instantiate(&mut store, &part)
                .expect("instantiate translated code part")
                .start(&mut store)
                .expect("start translated code part");
        }
        (store, instance)
    }

    fn translated_registers(program: &[u8]) -> [u64; 13] {
        let (mut store, instance) = translated_instance(program);
        let begin = instance
            .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
            .expect("translated begin export");
        assert_eq!(
            begin
                .call(&mut store, (0, i64::MAX))
                .expect("run translated fixture"),
            STATUS_FINISHED
        );
        register_values(&store, instance)
    }

    fn register_values(store: &Store<()>, instance: Instance) -> [u64; 13] {
        std::array::from_fn(|index| {
            let value = instance
                .get_global(store, &format!("r{index}"))
                .expect("translated register")
                .get(store);
            let Val::I64(value) = value else {
                panic!("translated register is not i64");
            };
            value as u64
        })
    }

    fn assert_differential_isa(isa: InstructionSetKind, source: &str) {
        let program = assemble(Some(isa), source).expect("assemble differential fixture");
        assert_eq!(
            translated_registers(&program),
            interpreter_registers(&program)
        );
    }

    fn assert_differential(source: &str) {
        assert_differential_isa(InstructionSetKind::Latest64, source);
    }

    #[test]
    fn framebuffer_fixture_translates_to_valid_wasm() {
        let program = include_bytes!("../tests/fixtures/framebuffer-test.polkavm");
        let wasm = translate(program).expect("translate framebuffer fixture");
        wasmparser::Validator::new()
            .validate_all(&wasm)
            .expect("validate translated framebuffer fixture");
        for part in code_parts(&wasm) {
            wasmparser::Validator::new()
                .validate_all(part)
                .expect("validate translated framebuffer code part");
        }
        instance_from_wasm(&wasm);
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

    #[test]
    fn arithmetic_matches_interpreter() {
        assert_differential(
            r#"
                %stack_size = 4096
                pub @main:
                a0 = 1234
                a1 = 100
                a2 = a0 + a1
                a3 = a2 * 7
                a4 = a3 /u a1
                a5 = a3 %u a1
                ret
            "#,
        );
    }

    #[test]
    fn reverse_bytes_match_interpreter() {
        let source = r#"
            %stack_size = 4096
            pub @main:
            a0 = 305419896
            a1 = reverse a0
            ret
        "#;
        assert_differential_isa(InstructionSetKind::Latest64, source);
        assert_differential_isa(InstructionSetKind::Latest32, source);
    }

    #[test]
    fn division_edges_match_interpreter() {
        assert_differential(
            r#"
                %stack_size = 4096
                pub @main:
                a0 = -2147483648
                a1 = -1
                i32 a2 = a0 /s a1
                i32 a3 = a0 %s a1
                a4 = 0
                a5 = a0 /u a4
                ret
            "#,
        );
    }

    #[test]
    fn memory_and_branches_match_interpreter() {
        assert_differential(
            r#"
                %rw_data_size = 65536
                %stack_size = 4096
                pub @main:
                a0 = 305419896
                u64 [131072] = a0
                a1 = u8 [131072]
                a2 = u16 [131072]
                a3 = i32 [131072]
                a4 = u64 [131072]
                a5 = 131072
                u64 [a5 + 8] = a0
                t0 = u64 [a5 + 8]
                a5 = sp - 16
                u64 [sp + -16] = t0
                t1 = u64 [a5]
                u64 [a5] = a1
                t2 = u64 [sp + -16]
                jump @matched if a1 == 120
                a5 = -1
                ret
                @matched:
                a5 = a4
                ret
            "#,
        );
    }

    #[test]
    fn narrow_memory_widths_and_signs_match_interpreter() {
        let source = r#"
            %rw_data_size = 65536
            %stack_size = 4096
            pub @main:
            a5 = 131073
            u32 [a5] = -1
            a0 = i8 [a5]
            a1 = u8 [a5]
            a2 = i16 [a5]
            a3 = u16 [a5]
            a4 = i32 [a5]
            t0 = u32 [a5]
            u8 [a5] = a3
            u8 [a5 + 1] = 128
            t1 = u16 [a5]
            u16 [a5] = a4
            u16 [a5 + 2] = 32768
            t2 = u32 [a5]
            u32 [a5] = a5
            a5 = u32 [a5]
            ret
        "#;
        assert_differential_isa(InstructionSetKind::Latest64, source);
        assert_differential_isa(
            InstructionSetKind::Latest32,
            &source.replace("= u32 [", "= i32 ["),
        );
    }

    #[test]
    fn wide_memory_preserves_high_bits_and_aliases() {
        assert_differential(
            r#"
                %rw_data_size = 65536
                %stack_size = 4096
                pub @main:
                a5 = 131073
                u64 [a5] = -2147483648
                a0 = u64 [a5]
                u64 [sp + -9] = a0
                a1 = u64 [sp + -9]
                u64 [a5] = a5
                a5 = u64 [a5]
                a2 = u64 [131073]
                ret
            "#,
        );
    }

    #[test]
    fn memory_guards_report_source_pc_without_writing_destination() {
        // Static width checking, static write protection, and dynamic write
        // protection must still return before the shared memory access runs.
        for operation in ["a0 = u64 [196604]", "u8 [65536] = a0", "u32 [a1] = a0"] {
            let source = format!(
                r#"
                    %rw_data_size = 65536
                    %stack_size = 4096
                    pub @main:
                    a0 = 7
                    {operation}
                    a0 = 99
                    ret
                "#
            );
            let program = assemble(Some(InstructionSetKind::Latest64), &source).unwrap();
            let blob = ProgramBlob::parse((&program[..]).into()).unwrap();
            let trap_pc = blob.instructions().nth(1).unwrap().offset.0;
            let (mut store, instance) = translated_instance(&program);
            instance
                .get_global(&store, Reg::A1.name_non_abi())
                .unwrap()
                .set(&mut store, Val::I64(65536))
                .unwrap();
            let begin = instance
                .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
                .unwrap();
            assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_TRAP);
            assert_eq!(
                instance
                    .get_global(&store, "trap_pc")
                    .unwrap()
                    .get(&store)
                    .i32(),
                Some(trap_pc as i32),
            );
            assert_eq!(
                instance
                    .get_global(&store, Reg::A0.name_non_abi())
                    .unwrap()
                    .get(&store)
                    .i64(),
                Some(7),
            );
        }
    }

    #[test]
    fn latest32_unsigned_control_flow_matches_interpreter() {
        assert_differential_isa(
            InstructionSetKind::Latest32,
            r#"
                %stack_size = 4096
                pub @main:
                a0 = -1
                a1 = 1
                i32 a2 = a0 + a1
                jump @wrong if a0 <u a1
                a3 = 7
                ret
                @wrong:
                a3 = 9
                ret
            "#,
        );
    }

    #[test]
    fn grouped_direct_and_indirect_jumps_match_interpreter() {
        let count = 2 * BLOCKS_PER_FUNCTION as u32 + 1;
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest64);
        builder.set_stack_size(4096);
        builder.add_export_by_basic_block(0, b"main");
        builder.add_export_by_basic_block(count - 1, b"tail");
        let mut instructions = Vec::new();
        for index in 0..count {
            instructions.push(asm::add_imm_64(Reg::A0, Reg::A0, 1));
            if index + 1 == count {
                instructions.push(asm::ret());
            } else if index % 2 == 0 {
                instructions.push(asm::load_imm(Reg::A1, (2 * (index + 2)) as i32));
                instructions.push(asm::jump_indirect(Reg::A1, 0));
            } else {
                instructions.push(asm::jump(index + 1));
            }
        }
        builder.set_code(&instructions, &(0..count).collect::<Vec<_>>());
        let program = builder.into_vec().unwrap();
        assert_eq!(
            translated_registers(&program),
            interpreter_registers(&program)
        );

        let wasm = translate(&program).unwrap();
        for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
            match payload.unwrap() {
                wasmparser::Payload::CustomSection(section)
                    if section.name() == "epoca.pvm.meta" =>
                {
                    // The final EPM2 export is tail. Its token must retain both
                    // the group and the slot, not merely name a Wasm function.
                    let data = section.data();
                    let entry = i32::from_le_bytes(data[data.len() - 4..].try_into().unwrap());
                    let (mut store, instance) = translated_instance(&program);
                    let begin = instance
                        .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
                        .unwrap();
                    assert_eq!(begin.call(&mut store, (entry, 1)).unwrap(), STATUS_FINISHED);
                    assert_eq!(
                        instance
                            .get_global(&store, Reg::A0.name_non_abi())
                            .unwrap()
                            .get(&store)
                            .i64(),
                        Some(1),
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn cross_part_loop_shares_memory_and_registers() {
        // Exercise forward and backward control flow both within a module and
        // across parts, including loads through local helpers and shared stores.
        let mut source = String::from(
            "%stack_size = 4096\npub @main:\ni32 a5 = sp - 16\n\
             u32 [a5] = 0\na1 = 0\njump @block0\n",
        );
        for index in 0..BLOCKS_PER_FUNCTION {
            source.push_str(&format!(
                "@block{index}:\na0 = i32 [a5]\ni32 a0 = a0 + 1\n\
                 u32 [a5] = a0\njump @block{}\n",
                index + 1,
            ));
        }
        source.push_str(&format!(
            "@block{BLOCKS_PER_FUNCTION}:\ni32 a1 = a1 + 1\n\
             jump @block0 if a1 <u 3\na2 = i32 [a5]\nret\n",
        ));
        for isa in [InstructionSetKind::Latest32, InstructionSetKind::Latest64] {
            let program = assemble(Some(isa), &source).unwrap();
            let wasm = translate_with_part_limit(&program, Some(1)).unwrap();
            assert_eq!(code_parts(&wasm).count(), 2, "fixture must cross parts");
            for wasm in [
                translate(&program).unwrap(),
                translate_partitioned(&program).unwrap(),
                wasm,
            ] {
                let (mut store, instance) = instance_from_wasm(&wasm);
                let begin = instance
                    .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
                    .unwrap();
                assert_eq!(
                    begin.call(&mut store, (0, i64::MAX)).unwrap(),
                    STATUS_FINISHED,
                );
                let registers = register_values(&store, instance);
                assert_eq!(registers, interpreter_registers(&program));
                assert_eq!(
                    registers[Reg::A2.to_u32() as usize],
                    3 * BLOCKS_PER_FUNCTION as u64
                );
            }
        }
    }

    #[test]
    fn long_straight_line_block_crosses_groups_without_charging_gas() {
        let count = BLOCKS_PER_FUNCTION * INSTRUCTIONS_PER_BLOCK + 1;
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest64);
        builder.set_stack_size(4096);
        builder.add_export_by_basic_block(0, b"main");
        let mut instructions = vec![asm::add_imm_64(Reg::A0, Reg::A0, 1); count];
        instructions.push(asm::ret());
        builder.set_code(&instructions, &[]);
        let program = builder.into_vec().unwrap();
        assert_eq!(
            translated_registers(&program),
            interpreter_registers(&program)
        );
        let (mut store, instance) = translated_instance(&program);
        let begin = instance
            .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
            .unwrap();
        assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_FINISHED);
        assert_eq!(
            instance
                .get_global(&store, Reg::A0.name_non_abi())
                .unwrap()
                .get(&store)
                .i64(),
            Some(count as i64),
        );
    }

    #[test]
    fn gas_resume_does_not_restart_the_entrypoint() {
        let program = assemble(
            Some(InstructionSetKind::Latest64),
            r#"
            %stack_size = 4096
            pub @main:
            a0 = 0
            jump @loop
            @loop:
            a0 = a0 + 1
            jump @loop if a0 <u 3
            ret
        "#,
        )
        .unwrap();
        let (mut store, instance) = translated_instance(&program);
        let begin = instance
            .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
            .unwrap();
        let resume = instance
            .get_typed_func::<(), i32>(&store, "pvm_resume")
            .unwrap();
        let gas = instance
            .get_typed_func::<i64, ()>(&store, "pvm_set_gas")
            .unwrap();
        assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_OUT_OF_GAS);
        gas.call(&mut store, 3).unwrap();
        assert_eq!(resume.call(&mut store, ()).unwrap(), STATUS_OUT_OF_GAS);
        assert_eq!(
            instance
                .get_global(&store, Reg::A0.name_non_abi())
                .unwrap()
                .get(&store)
                .i64(),
            Some(2),
        );
        gas.call(&mut store, 2).unwrap();
        assert_eq!(resume.call(&mut store, ()).unwrap(), STATUS_FINISHED);
        assert_eq!(
            instance
                .get_global(&store, Reg::A0.name_non_abi())
                .unwrap()
                .get(&store)
                .i64(),
            Some(3),
        );
    }

    #[test]
    fn indirect_gas_resume_preserves_fused_jump_side_effects() {
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest64);
        builder.set_stack_size(4096);
        builder.add_export_by_basic_block(0, b"main");
        let mut instructions = vec![
            asm::jump(2),
            asm::add_imm_64(Reg::A0, Reg::A0, 1),
            asm::ret(),
        ];
        instructions.extend(std::iter::repeat_n(
            asm::add_imm_64(Reg::A2, Reg::A2, 1),
            INSTRUCTIONS_PER_BLOCK + 1,
        ));
        instructions.push(asm::load_imm(Reg::A1, 2));
        instructions.push(asm::load_imm_and_jump_indirect(Reg::A1, Reg::A1, 999, 0));
        builder.set_code(&instructions, &[1]);
        let program = builder.into_vec().unwrap();
        let (mut store, instance) = translated_instance(&program);
        let begin = instance
            .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
            .unwrap();
        let resume = instance
            .get_typed_func::<(), i32>(&store, "pvm_resume")
            .unwrap();
        let gas = instance
            .get_typed_func::<i64, ()>(&store, "pvm_set_gas")
            .unwrap();
        assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_OUT_OF_GAS);
        gas.call(&mut store, 2).unwrap();
        assert_eq!(resume.call(&mut store, ()).unwrap(), STATUS_FINISHED);
        for (reg, value) in [
            (Reg::A0, 1),
            (Reg::A1, 999),
            (Reg::A2, INSTRUCTIONS_PER_BLOCK as i64 + 1),
        ] {
            assert_eq!(
                instance
                    .get_global(&store, reg.name_non_abi())
                    .unwrap()
                    .get(&store)
                    .i64(),
                Some(value),
            );
        }
    }

    #[test]
    fn hostcall_resume_across_groups_reports_exact_trap_pc() {
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest64);
        builder.set_stack_size(4096);
        builder.add_export_by_basic_block(0, b"main");
        builder.add_import(b"host_test");
        let count = BLOCKS_PER_FUNCTION * INSTRUCTIONS_PER_BLOCK;
        let mut instructions = vec![asm::add_imm_64(Reg::A0, Reg::A0, 1); count];
        instructions.push(asm::ecalli(0));
        instructions.push(asm::trap());
        builder.set_code(&instructions, &[]);
        let program = builder.into_vec().unwrap();
        let blob = ProgramBlob::parse((&program[..]).into()).unwrap();
        let trap_pc = blob
            .instructions()
            .find(|instruction| instruction.kind == asm::trap())
            .unwrap()
            .offset
            .0;
        let (mut store, instance) = translated_instance(&program);
        let begin = instance
            .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
            .unwrap();
        let resume = instance
            .get_typed_func::<(), i32>(&store, "pvm_resume")
            .unwrap();
        assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_ECALL);
        assert_eq!(resume.call(&mut store, ()).unwrap(), STATUS_TRAP);
        assert_eq!(
            instance
                .get_global(&store, "trap_pc")
                .unwrap()
                .get(&store)
                .i32(),
            Some(trap_pc as i32),
        );
        assert_eq!(
            instance
                .get_global(&store, Reg::A0.name_non_abi())
                .unwrap()
                .get(&store)
                .i64(),
            Some(count as i64),
        );
    }

    #[test]
    fn indirect_traps_report_the_source_instruction_pc() {
        for base in [Reg::A1, Reg::RA] {
            let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest64);
            builder.set_stack_size(4096);
            builder.add_export_by_basic_block(0, b"main");
            builder.set_code(
                &[
                    asm::load_imm(Reg::A0, 7),
                    asm::move_reg(base, Reg::A2),
                    asm::jump_indirect(base, 0),
                    asm::ret(),
                ],
                &[1],
            );
            let program = builder.into_vec().unwrap();
            let blob = ProgramBlob::parse((&program[..]).into()).unwrap();
            let trap_pc = blob.instructions().nth(2).unwrap().offset.0;
            let (mut store, instance) = translated_instance(&program);
            let begin = instance
                .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
                .unwrap();
            let target = instance.get_global(&store, Reg::A2.name_non_abi()).unwrap();
            // Exercise both invalid-address paths in the jump and return helpers.
            for address in [3, 4] {
                target.set(&mut store, Val::I64(address)).unwrap();
                assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_TRAP);
                assert_eq!(
                    instance
                        .get_global(&store, "trap_pc")
                        .unwrap()
                        .get(&store)
                        .i32(),
                    Some(trap_pc as i32),
                );
                assert_eq!(
                    instance
                        .get_global(&store, Reg::A0.name_non_abi())
                        .unwrap()
                        .get(&store)
                        .i64(),
                    Some(7),
                );
            }
        }
    }

    #[test]
    fn indirect_group_padding_is_not_a_valid_jump_target() {
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest64);
        builder.set_stack_size(4096);
        builder.add_export_by_basic_block(0, b"main");
        builder.set_code(&[asm::jump_indirect(Reg::A1, 0), asm::ret()], &vec![1; 129]);
        let program = builder.into_vec().unwrap();
        let (mut store, instance) = translated_instance(&program);
        let begin = instance
            .get_typed_func::<(i32, i64), i32>(&store, "pvm_begin")
            .unwrap();
        let target = instance.get_global(&store, Reg::A1.name_non_abi()).unwrap();
        for address in [0, 1, 260] {
            target.set(&mut store, Val::I64(address)).unwrap();
            assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_TRAP);
            assert_eq!(
                instance
                    .get_global(&store, "trap_pc")
                    .unwrap()
                    .get(&store)
                    .i32(),
                Some(0)
            );
        }
        for address in [2, 258, RETURN_TO_HOST as i64] {
            target.set(&mut store, Val::I64(address)).unwrap();
            assert_eq!(begin.call(&mut store, (0, 1)).unwrap(), STATUS_FINISHED);
        }
    }
}
