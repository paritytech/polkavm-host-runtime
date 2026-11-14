import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

await import("../src/polkavm-wasm-translated.js");
const runtime = new WebAssembly.Module(await readFile(new URL("../dist/polkavm-browser-runtime.wasm", import.meta.url)));
const imports = { polkavm_browser: Object.fromEntries(WebAssembly.Module.imports(runtime)
  .map(({ name }) => [name, () => { throw new Error(`unexpected translator hostcall ${name}`); }])) };
const create = (assets = new Map(), now = () => performance.now()) =>
  globalThis.TranslatedPolkaVmRuntime.createValidation(runtime, imports, assets, now);
const text = (value) => [...new TextEncoder().encode(value)];
const imm = (reg, value) => [51, reg, value & 255, (value >>> 8) & 255, (value >>> 16) & 255, value >>> 24];
const call = (index) => [10, index];
const ret = [50, 0];
const RO = 0x10000;
const RW = 0x30000;
const LIMIT = 1048512;
const varint = (value) => {
  assert.ok(value < 16384, "small fixture integer");
  return value < 128 ? [value] : [0x80 | (value >>> 8), value & 255];
};
const word = (value) => [value & 255, (value >>> 8) & 255, (value >>> 16) & 255, value >>> 24];
const incrementCounter = [128, 0x66, ...word(1)]; // add_imm_64 t2, t2, 1
const counterLessThan = (count, relativeTarget) => [41, 0x46, ...word(count), ...word(relativeTarget)];
const returnCounter = [227, 0x67]; // move a0, t2

// Real Latest64 bytecode, using the same small-blob encoding as runtime-core tests.
function program(names, instructions, data = [1, 2, 3, 4], bitness = 64, entry = "validate") {
  const section = (id, bytes) => [id, ...varint(bytes.length), ...bytes];
  const symbols = [], offsets = [];
  for (const name of names) {
    offsets.push(symbols.length, 0, 0, 0);
    symbols.push(...text(name));
  }
  if (bitness === 64) {
    instructions = instructions.map(([opcode, ...args]) => [
      ({ 51: 8, 10: 221, 50: 3 })[opcode] ?? opcode, ...args,
    ]);
  }
  const code = instructions.flat();
  const mask = new Uint8Array(Math.ceil(code.length / 8));
  let offset = 0;
  for (const instruction of instructions) {
    mask[offset >> 3] |= 1 << (offset & 7);
    offset += instruction.length;
  }
  const bytes = new Uint8Array([
    ...text("PVM\0"), bitness === 64 ? 2 : 1, ...new Array(8).fill(0),
    ...section(1, [...varint(data.length), 64, 0]),
    ...section(2, data),
    ...(names.length ? section(4, [names.length, ...offsets, ...symbols]) : []),
    ...section(5, [1, 0, entry.length, ...text(entry)]),
    ...section(6, [0, 0, ...varint(code.length), ...code, ...(bitness === 32 ? mask : [])]), 0,
  ]);
  new DataView(bytes.buffer).setBigUint64(5, BigInt(bytes.length), true);
  return bytes;
}
function loaded(bytes, assets, now) {
  const child = create(assets, now);
  assert.equal(child.load(bytes, 2), 0);
  child.beginUpdate();
  child.start(new Uint8Array());
  return child;
}

test("validation load enforces version, RV64, imports, entry, and single load", () => {
  const bytes = program([], [imm(7, 42), ret]);
  const child = create();
  assert.throws(() => child.load(bytes, 1), /version/);
  assert.throws(() => child.load(program([], [ret], [], 32), 2), /RV64/);
  assert.throws(() => child.load(program(["host_log"], [ret]), 2), /unknown child import/);
  assert.throws(() => child.load(program([], [ret], [], 64, "other"), 2), /validate export/);
  child.load(bytes, 2);
  assert.throws(() => child.load(bytes, 2), /already loaded/);
  assert.throws(() => child.start(new Uint8Array(LIMIT + 1)), /checkpoint exceeds/);
  child.start(new Uint8Array());
  assert.throws(() => child.start(new Uint8Array()), /already active/);
  assert.equal(child.step(), 0, "no execution before host budget refill");
  child.beginUpdate();
  assert.equal(child.step(), 4);
  assert.equal(new DataView(child.read(0, 8).buffer).getBigUint64(0, true), 42n);
  assert.equal(child.work(), 0n, "straight-line return has zero translated loop gas");
  assert.throws(() => child.step(), /finished/);
  child.cancel();
  assert.equal(child.run, null);
  assert.throws(() => child.step(), /not started/);
  assert.throws(() => child.load(bytes, 2), /already loaded/);
  child.start(new Uint8Array());
  assert.equal(child.step(), 4);
});

test("checkpoint commit blocks execution until ACK and resumes exact return", () => {
  const child = loaded(program(["validation_checkpoint_write", "validation_checkpoint_commit"], [
    imm(7, RO), imm(8, 4), call(0), call(1), ret,
  ]));
  assert.equal(child.step(), 3);
  assert.deepEqual([...child.read(0, 4096)], [1, 2, 3, 4]);
  assert.throws(() => child.step(), /durable ACK/);
  assert.throws(() => child.read(0, 4097), /chunk limit/);
  assert.throws(() => child.read(5, 1), /offset/);
  assert.equal(child.ack(), 0);
  assert.deepEqual([...child.read(0, 4096)], [1, 2, 3, 4]);
  assert.throws(() => child.ack(), /without pending/);
  assert.equal(child.step(), 4);
  assert.equal(new DataView(child.read(0, 8).buffer).getBigUint64(0, true), 0n);
});

test("checkpoint writes enforce total limit without publishing partial state", () => {
  const child = loaded(program(["validation_checkpoint_write"], [ret]));
  child.run.pvm.r7.value = BigInt(RO);
  child.run.pvm.r8.value = 4n;
  for (let i = 0; i < LIMIT / 4; i++) {
    if (child.dispatch("validation_checkpoint_write") === null) {
      child.beginUpdate();
      assert.equal(child.dispatch("validation_checkpoint_write"), 0);
    }
    child.run.pvm.r7.value = BigInt(RO);
  }
  assert.equal(child.run.stagedLength, LIMIT);
  assert.throws(() => child.dispatch("validation_checkpoint_write"), /checkpoint exceeds/);
  assert.equal(child.run.committed.length, 0);
});

test("peer request gates stepping, bounds replies and resumes with exact reply length", () => {
  const child = loaded(program(["validation_peer_request", "validation_peer_read", "validation_report"], [
    imm(7, RO), imm(8, 4), call(0),
    imm(7, 0), imm(8, RW), imm(9, 4), call(1),
    imm(7, RW), imm(8, 4), call(2), ret,
  ]));
  assert.throws(() => child.peerWrite(new Uint8Array([1])), /without pending/);
  assert.equal(child.step(), 5);
  assert.deepEqual([...child.read(0, 4)], [1, 2, 3, 4]);
  assert.throws(() => child.step(), /requires response/);
  assert.throws(() => child.peerCommit(), /without pending response/);
  assert.throws(() => child.peerWrite(new Uint8Array(4097)), /chunk bounds/);
  assert.throws(() => child.peerWrite(new Uint8Array()), /chunk bounds/);
  const reply = new Uint8Array(4096).fill(7);
  for (let i = 0; i < 255; i++) child.peerWrite(reply);
  child.peerWrite(reply.subarray(0, 4032));
  assert.throws(() => child.peerWrite(reply.subarray(0, 1)), /exceeds limit/);
  child.peerCommit();
  assert.equal(child.run.pvm.r7.value, BigInt(LIMIT));
  assert.equal(child.step(), 2);
  assert.deepEqual([...child.read(0, 4)], [7, 7, 7, 7]);
  assert.equal(child.step(), 4);
});

test("asset whitelist rejects unknown names and peer waits use the supplied monotonic clock", () => {
  const child = loaded(program(["validation_read"], [imm(7, RO), imm(8, 4), imm(9, 0), imm(10, RW), imm(11, 4), call(0), ret], text("nope")), new Map([["nope", new Uint8Array(4)]]));
  assert.throws(() => child.step(), /refused asset name/);
  let now = 100;
  const waiting = loaded(program(["validation_peer_wait", "validation_peer_now_ms"], [imm(7, 25), call(0), call(1), ret]), undefined, () => now);
  assert.equal(waiting.step(), 6);
  assert.equal(new DataView(waiting.read(0, 4).buffer).getUint32(0, true), 25);
  const work = waiting.work();
  now = 124;
  assert.equal(waiting.step(), 6);
  assert.equal(waiting.work(), work);
  now = 125;
  assert.equal(waiting.step(), 4);
  assert.equal(new DataView(waiting.read(0, 8).buffer).getBigUint64(0, true), 25n);
});

test("interpreter rejects cooperative and CoreVM host_validation imports before execution", () => {
  for (const entry of ["init", "_pvm_start"]) {
    const pvm = new WebAssembly.Instance(runtime, imports).exports;
    const bytes = program(["host_validation_step"], [ret], [], 32, entry);
    const pointer = pvm.polkavm_browser_staging_reserve(bytes.length);
    new Uint8Array(pvm.memory.buffer, pointer, bytes.length).set(bytes);
    assert.equal(pvm.polkavm_browser_launch_begin(1000000n, 0), 0);
    assert.notEqual(pvm.polkavm_browser_launch_start(), 0);
    const message = new TextDecoder().decode(new Uint8Array(pvm.memory.buffer,
      pvm.polkavm_browser_error_pointer(), pvm.polkavm_browser_error_length()));
    assert.match(message, /host_validation.*translated browser backend.*interpreter/);
  }
});

// Optional external qualification input; no dependency on a live seat or network.
// Set both paths to run the independently linked JAM finite-replay computation.
test("RV64 JAM probe produces byte-identical finite replay report", {
  skip: !process.env.HOST_VALIDATION_PROBE || !process.env.HOST_VALIDATION_FIXTURES,
}, async () => {
  const assets = new Map(await Promise.all([
    "state.bin", "inputs.bin", "parameters.bin", "genesis-header.bin", "genesis-root.bin",
  ].map(async (name) => [name, new Uint8Array(await readFile(resolve(process.env.HOST_VALIDATION_FIXTURES, name)))])));
  const child = loaded(await readFile(process.env.HOST_VALIDATION_PROBE), assets);
  const reports = [];
  let checkpoints = 0, work = 0n;
  for (let steps = 0; ; steps++) {
    assert.ok(steps < 1000000, "finite replay must terminate");
    const event = child.step();
    assert.ok(child.work() >= work, "monotonic translated-module work");
    work = child.work();
    if (event === 0) { child.beginUpdate(); continue; }
    if (event === 1) continue;
    if (event === 2) { reports.push(child.read(0, 4096).slice()); continue; }
    if (event === 3) {
      assert.throws(() => child.step(), /durable ACK/);
      let length = 0;
      for (;;) {
        const chunk = child.read(length, 4096);
        length += chunk.length;
        if (chunk.length < 4096) break;
      }
      assert.ok(length > 0 && length <= LIMIT);
      checkpoints++;
      child.ack();
      continue;
    }
    assert.equal(event, 4);
    assert.equal(new DataView(child.read(0, 8).buffer).getBigUint64(0, true), 0n);
    break;
  }
  assert.ok(checkpoints > 0);
  assert.ok(work > 0n);
  assert.equal(createHash("sha256").update(Buffer.concat(reports)).digest("hex"),
    "2fdf4c08d0dfb0011e6d7a89aed84e8b025124fa6e39279ba6014389f55b6def");
});

test("translated RV32 wrapper loads, checkpoints, acknowledges and finishes its RV64 child", async () => {
  const child = program(["validation_checkpoint_write", "validation_checkpoint_commit"], [
    imm(7, RO), imm(8, 4), call(0), call(1), ret,
  ]);
  const bytes = program([
    "host_validation_load", "host_validation_start", "host_validation_step",
    "host_validation_read", "host_validation_ack", "host_validation_work",
    "host_validation_cancel", "host_save_submit",
  ], [
    imm(7, RO), imm(8, child.length), imm(9, 2), call(0),
    imm(7, 1), imm(8, 0), call(1), call(2), // empty Rust slice has a dangling pointer
    imm(7, 0), imm(8, RW), imm(9, 4), call(3),
    imm(7, RW), imm(8, 4), call(7),
    imm(7, 4), imm(8, 1), imm(9, 4), call(3), // EOF does not dereference destination
    call(4), call(2),
    imm(7, 0), imm(8, RW), imm(9, 8), call(3),
    imm(7, RW), imm(8, 8), call(7),
    call(5), call(6), ret,
  ], [...child], 32, "init");
  const translator = new WebAssembly.Instance(runtime, imports).exports;
  const pointer = translator.polkavm_browser_staging_reserve(bytes.length);
  new Uint8Array(translator.memory.buffer, pointer, bytes.length).set(bytes);
  assert.equal(translator.polkavm_browser_translate_staged(), 0);
  const wasm = new Uint8Array(translator.memory.buffer,
    translator.polkavm_browser_translation_pointer(), translator.polkavm_browser_translation_length());
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const outputs = [];
  const outer = new Runtime(await Runtime.compile(wasm), [],
    (output) => outputs.push(output), 1000000, false, "framebuffer",
    null, 0, [], null, 1, { runtime, imports });
  outer.initialize();
  assert.deepEqual(outputs.filter(({ type }) => type === "save").map(({ bytes }) => [...bytes]),
    [[1, 2, 3, 4], new Array(8).fill(0)]);
  assert.equal(outer.validation.run, null, "cancel releases child instance");
  outer.stop();
  assert.equal(outer.validation, null, "stop releases child code and translator references");
});

test("work slices are bounded and cannot be refilled by step, cancel or start", () => {
  const child = loaded(program([], [[217, 0]])); // RV64 jump to self.
  assert.equal(child.step(), 0);
  assert.equal(child.work(), 4194304n);
  assert.equal(child.step(), 0);
  assert.equal(child.work(), 4194304n);
  child.beginUpdate();
  assert.equal(child.step(), 0);
  assert.equal(child.work(), 8388608n);
  child.cancel();
  child.start(new Uint8Array());
  assert.equal(child.step(), 0);
  assert.equal(child.work(), 0n);
  assert.equal(child.metadata.layout.heapLimit, 64 * 1024 * 1024);
});

test("child clock hostcall loops yield at the per-update count limit and finish without replay", () => {
  const calls = 3 * 65536 + 1;
  let clockReads = 0;
  const child = loaded(program(["validation_peer_now_ms"], [
    imm(6, 0), call(0), incrementCounter, counterLessThan(calls, -8), returnCounter, ret,
  ]), undefined, () => { clockReads++; return 0; });
  let previousWork = 0n;
  for (let update = 0; update < 4; update++) {
    const before = clockReads;
    const event = child.step();
    const expected = update < 3 ? 65536 : 1;
    assert.equal(clockReads - before, expected);
    assert.equal(event, update < 3 ? 0 : 4);
    assert.ok(child.work() >= previousWork + BigInt(expected), "every hostcall costs work");
    previousWork = child.work();
    if (event === 0) {
      assert.equal(child.hostcalls, 0);
      assert.equal(child.step(), 0);
      assert.equal(clockReads - before, expected, "step cannot refill the call budget");
      assert.equal(child.work(), previousWork);
      child.beginUpdate();
    }
  }
  assert.equal(clockReads, calls + 1, "one start-clock sample plus exactly one per child call");
  assert.equal(new DataView(child.read(0, 8).buffer).getBigUint64(0, true), BigInt(calls));
});

test("child asset reads defer an unexecuted ECALL at the byte limit and resume it exactly once", () => {
  const calls = 20000;
  let reads = 0;
  const assets = new class extends Map {
    get(name) { reads++; return super.get(name); }
  }([["state.bin", new Uint8Array(4096).fill(7)]]);
  const child = loaded(program(["validation_read"], [
    imm(6, 0), imm(8, 9), imm(9, 0), imm(10, RW), imm(11, 4096),
    imm(7, RO), call(0), incrementCounter, counterLessThan(calls, -14), returnCounter, ret,
  ], text("state.bin")), assets);
  const perUpdate = Math.floor(32 * 1024 * 1024 / (4096 + 9));
  let previousWork = 0n;
  for (let update = 0; update < 3; update++) {
    const before = reads;
    const event = child.step();
    assert.equal(reads - before, update < 2 ? perUpdate : calls - 2 * perUpdate);
    assert.ok(child.work() > previousWork);
    previousWork = child.work();
    if (update < 2) {
      assert.equal(event, 0);
      assert.equal(child.run.callPending, true, "ECALL awaits next byte allowance");
      assert.equal(child.run.pvm.r7.value, BigInt(RO), "arguments remain intact");
      assert.equal(child.step(), 0);
      assert.equal(reads - before, perUpdate, "no partial copy or replay on repeated step");
      assert.equal(child.work(), previousWork);
      child.beginUpdate();
    } else {
      assert.equal(event, 4);
    }
  }
  assert.equal(reads, calls);
  assert.equal(new DataView(child.read(0, 8).buffer).getBigUint64(0, true), BigInt(calls));
  assert.deepEqual(child.range(RW, 4096), new Uint8Array(4096).fill(7));
});

test("child hostcall budgets survive cancel and restart until the next outer update", () => {
  const child = loaded(program(["validation_peer_now_ms"], [call(0), [217, 254]]));
  assert.equal(child.step(), 0);
  assert.equal(child.hostcalls, 0);
  child.cancel();
  child.start(new Uint8Array());
  assert.equal(child.step(), 0);
  assert.equal(child.work(), 0n);
  child.beginUpdate();
  assert.equal(child.step(), 0);
  assert.equal(child.hostcalls, 0);
  assert.ok(child.work() >= 65536n);
});
