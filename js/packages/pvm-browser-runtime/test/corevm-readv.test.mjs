import assert from "node:assert/strict";
import test from "node:test";

await import("../src/pvm-wasm-translated.js");

const encoder = new TextEncoder();
const RW_ADDRESS = 0x10000;
const MEMORY_BYTES = 0x10000;
const IOV_OFFSET = 64;
const OUTPUT_OFFSET = 512;

function leb(value) {
  const bytes = [];
  do {
    const byte = value & 0x7f;
    value >>>= 7;
    bytes.push(byte | (value ? 0x80 : 0));
  } while (value);
  return bytes;
}

function name(value) {
  const bytes = encoder.encode(value);
  return [...leb(bytes.length), ...bytes];
}

function u32(value) {
  return [value & 255, (value >>> 8) & 255, (value >>> 16) & 255, value >>> 24];
}

function metadataName(value) {
  const bytes = encoder.encode(value);
  return [bytes.length & 255, bytes.length >>> 8, ...bytes];
}

function section(id, bytes) {
  return [id, ...leb(bytes.length), ...bytes];
}

// A minimal executable guest with the translated ABI: begin yields one syscall,
// resume exits. The tests supply registers and iovecs, but all descriptor lookup,
// guest memory access, file reads, offsets, and errno handling use the real host.
function syscallModule() {
  const globals = Array.from({ length: 13 }, () => [0x7e, 1, 0x42, 0, 0x0b]);
  globals.push([0x7f, 1, 0x41, 0, 0x0b]);
  const exports = [
    [...name("memory"), 2, 0],
    ...Array.from({ length: 13 }, (_, index) => [...name(`r${index}`), 3, index]),
    [...name("ecall"), 3, 13],
    [...name("pvm_begin"), 0, 0],
    [...name("pvm_resume"), 0, 1],
    [...name("pvm_set_gas"), 0, 2],
  ];
  const bodies = [
    [0, 0x41, 0x7e, 0x0b], // i32.const -2 (ECALL)
    [0, 0x41, 0x7f, 0x0b], // i32.const -1 (FINISHED)
    [0, 0x0b],
  ];
  const layout = [
    0, 0, 0, // no read-only segment
    RW_ADDRESS, MEMORY_BYTES, 0,
    RW_ADDRESS + MEMORY_BYTES, RW_ADDRESS + MEMORY_BYTES,
    RW_ADDRESS + 0xf000, RW_ADDRESS + MEMORY_BYTES, 0xf000,
  ];
  const metadata = [
    ...encoder.encode("EPM2"), ...u32(1),
    ...layout.flatMap(u32),
    ...u32(1), ...metadataName("pvm_syscall"),
    ...u32(1), ...metadataName("_pvm_start"), ...u32(0),
  ];
  return new WebAssembly.Module(new Uint8Array([
    0, 0x61, 0x73, 0x6d, 1, 0, 0, 0,
    ...section(0, [...name("epoca.pvm.meta"), ...metadata]),
    ...section(1, [
      3,
      0x60, 2, 0x7f, 0x7e, 1, 0x7f,
      0x60, 0, 1, 0x7f,
      0x60, 1, 0x7e, 0,
    ]),
    ...section(3, [3, 0, 1, 2]),
    ...section(5, [1, 0, 1]),
    ...section(6, [globals.length, ...globals.flat()]),
    ...section(7, [exports.length, ...exports.flat()]),
    ...section(10, [bodies.length, ...bodies.flatMap((body) => [...leb(body.length), ...body])]),
  ]));
}

const module = syscallModule();

function guest(bytes) {
  const runtime = new globalThis.TranslatedPvmRuntime(
    module,
    [{ path: "input.bin", bytes: new Uint8Array(bytes) }],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  runtime.initialize();
  const memory = new Uint8Array(runtime.memory.buffer);
  const view = new DataView(memory.buffer);
  const syscall = (number, ...args) => {
    [number, ...args].forEach((value, index) => {
      runtime.pvm[`r${index + 7}`].value = BigInt(value);
    });
    assert.throws(() => runtime.update(0), Error);
    return runtime.pvm.r7.value;
  };
  memory.set(encoder.encode("input.bin\0"), 16);
  const fd = syscall(56, -100, RW_ADDRESS + 16, 0);
  assert.equal(fd, 3n);
  memory.fill(0xcc, OUTPUT_OFFSET, OUTPUT_OFFSET + 32);
  return {
    memory,
    vector(index, address, length, offset = IOV_OFFSET) {
      view.setBigUint64(offset + index * 16, BigInt(address), true);
      view.setBigUint64(offset + index * 16 + 8, BigInt(length), true);
    },
    readv(count, offset = IOV_OFFSET) {
      return syscall(65, fd, RW_ADDRESS + offset, count);
    },
    read(length) {
      return syscall(63, fd, RW_ADDRESS + OUTPUT_OFFSET + 16, length);
    },
  };
}

test("CoreVM readv returns bytes actually read and stops before later vectors on EOF", () => {
  const runtime = guest([1, 2, 3]);
  runtime.vector(0, RW_ADDRESS + OUTPUT_OFFSET, 5);
  runtime.vector(1, 0, 1); // Must not be visited after the short first read.
  assert.equal(runtime.readv(2), 3n);
  assert.deepEqual(runtime.memory.slice(OUTPUT_OFFSET, OUTPUT_OFFSET + 6), new Uint8Array([1, 2, 3, 0xcc, 0xcc, 0xcc]));
  assert.equal(runtime.readv(2), 0n);
});

test("CoreVM readv totals complete vectors plus a short final read", () => {
  const runtime = guest([1, 2, 3, 4, 5]);
  runtime.vector(0, RW_ADDRESS + OUTPUT_OFFSET, 2);
  runtime.vector(1, RW_ADDRESS + OUTPUT_OFFSET + 4, 6);
  assert.equal(runtime.readv(2), 5n);
  assert.deepEqual(runtime.memory.slice(OUTPUT_OFFSET, OUTPUT_OFFSET + 8), new Uint8Array([1, 2, 0xcc, 0xcc, 3, 4, 5, 0xcc]));
  assert.equal(runtime.read(1), 0n);
});

test("CoreVM readv preserves partial progress when a later buffer faults", () => {
  const runtime = guest([1, 2, 3, 4, 5]);
  runtime.vector(0, RW_ADDRESS + OUTPUT_OFFSET, 2);
  runtime.vector(1, 0, 2);
  assert.equal(runtime.readv(2), 2n);
  assert.deepEqual(runtime.memory.slice(OUTPUT_OFFSET, OUTPUT_OFFSET + 3), new Uint8Array([1, 2, 0xcc]));
  assert.equal(runtime.read(5), 3n);
  assert.deepEqual(runtime.memory.slice(OUTPUT_OFFSET + 16, OUTPUT_OFFSET + 20), new Uint8Array([3, 4, 5, 0xcc]));
});

test("CoreVM readv preserves partial progress when a later iovec descriptor faults", () => {
  const runtime = guest([1, 2, 3]);
  runtime.vector(0, RW_ADDRESS + OUTPUT_OFFSET, 2, MEMORY_BYTES - 16);
  assert.equal(runtime.readv(2, MEMORY_BYTES - 16), 2n);
  assert.equal(runtime.read(2), 1n);
  assert.equal(runtime.memory[OUTPUT_OFFSET + 16], 3);
});

test("CoreVM readv returns EFAULT without consuming data when no bytes were read", () => {
  const runtime = guest([1, 2, 3]);
  assert.equal(runtime.readv(1, MEMORY_BYTES), -14n);
  runtime.vector(0, RW_ADDRESS + OUTPUT_OFFSET, 0);
  runtime.vector(1, 0, 1);
  assert.equal(runtime.readv(2), -14n);
  assert.equal(runtime.read(3), 3n);
  assert.deepEqual(runtime.memory.slice(OUTPUT_OFFSET + 16, OUTPUT_OFFSET + 19), new Uint8Array([1, 2, 3]));
});
