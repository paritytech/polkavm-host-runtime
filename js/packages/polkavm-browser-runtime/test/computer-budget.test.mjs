/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

import assert from "node:assert/strict";
import test from "node:test";

await import("../src/polkavm-computer.js");
const { ComputerSupervisor, computerContext } = globalThis.PolkaVmComputer;
const encoder = new TextEncoder();
const ADDRESS = 0x10000;
const BUFFER = ADDRESS + 64;
const BUDGET = 8192;
const REQUESTS = 10_000;

function leb(value) {
  const bytes = [];
  do {
    const byte = value & 0x7f;
    value >>>= 7;
    bytes.push(byte | (value ? 0x80 : 0));
  } while (value);
  return bytes;
}

function signed(value) {
  const bytes = [];
  for (;;) {
    const byte = value & 0x7f;
    value >>= 7;
    const done = (value === 0 && !(byte & 0x40)) || (value === -1 && (byte & 0x40));
    bytes.push(byte | (done ? 0 : 0x80));
    if (done) return bytes;
  }
}

const i32 = (value) => [0x41, ...signed(value)];
const i64 = (value) => [0x42, ...signed(value)];
const get = (index) => [0x23, index];
const set = (index, value) => [...value, 0x24, index];
const u32 = (value) => [value & 255, (value >>> 8) & 255, (value >>> 16) & 255, value >>> 24];
const name = (value) => [...leb(encoder.encode(value).length), ...encoder.encode(value)];
const metadataName = (value) => [value.length & 255, value.length >>> 8, ...encoder.encode(value)];
const section = (id, bytes) => [id, ...leb(bytes.length), ...bytes];

// Executable guests using the translated ABI, as in corevm-readv.test.mjs.
// Each stage is a real Wasm function; its program counter survives host turns.
// No process/supervisor methods are replaced. Loops are finite even before the
// fix, so failures are assertions rather than hangs or timing-dependent tests.
function guest(build) {
  const stages = [];
  const imports = [];
  const pc = 14;
  const counter = 15;
  const emit = (code, terminal = false) => stages.push({ code, terminal });
  const call = (method, args = [], prefix = []) => {
    const symbol = `polkadot_host_0_1_${method}`;
    if (!imports.includes(symbol)) imports.push(symbol);
    emit([
      ...prefix,
      // Set a0 last so later arguments can consume the previous return value.
      ...args.map((value, index) => set(7 + index, Array.isArray(value) ? value : i64(value))).reverse().flat(),
      ...set(13, i32(imports.indexOf(symbol))),
      ...i32(-2),
    ], true);
  };
  const repeat = (count, body) => {
    emit(set(counter, i32(count)));
    const start = stages.length;
    body();
    stages.at(-1).next = [
      ...set(counter, [...get(counter), ...i32(1), 0x6b]),
      ...i32(start), ...i32(stages.length), ...get(counter), 0x1b,
      0x24, pc,
    ];
  };
  const report = () => call("tty_write", [1, BUFFER, 1], [
    ...i32(64), ...get(7), 0xa7, 0x3a, 0, 0,
  ]);
  const launch = (method, offset = 0, length = 6) =>
    call(method, [ADDRESS + offset, length, 0, 0, 80, 24]);
  const saveHandle = () => emit(set(0, get(7)));
  const forward = () => call("tty_write", [1, BUFFER, get(7)]);
  const exit = (code = 23) => call("core_exit", [code]);
  const trap = () => emit([0x00], true);
  build({ call, repeat, report, launch, saveHandle, forward, exit, trap });

  const globals = Array.from({ length: 13 }, () => [0x7e, 1, ...i64(0), 0x0b]);
  globals.push(...Array.from({ length: 3 }, () => [0x7f, 1, ...i32(0), 0x0b]));
  const exports = [
    [...name("memory"), 2, 0],
    ...Array.from({ length: 13 }, (_, index) => [...name(`r${index}`), 3, index]),
    [...name("ecall"), 3, 13],
    [...name("pvm_begin"), 0, 0],
    [...name("pvm_resume"), 0, 1],
    [...name("pvm_set_gas"), 0, 2],
  ];
  const bodies = [
    [0, 0x10, 1, 0x0b],
    [0, ...get(pc), 0x11, 1, 0, 0x0b],
    [0, 0x0b],
    ...stages.map((stage, index) => [
      0,
      ...(stage.next ?? set(pc, i32(index + 1))),
      ...stage.code,
      // Non-hostcall setup stages immediately continue to the next stage.
      ...(stage.terminal ? [] : [0x10, 1]),
      0x0b,
    ]),
  ];
  const layout = [0, 0, 0, ADDRESS, 0x10000, 0, ADDRESS + 0x10000,
    ADDRESS + 0x10000, ADDRESS + 0xf000, ADDRESS + 0x10000, 0xf000];
  const metadata = [
    ...encoder.encode("EPM2"), ...u32(1), ...layout.flatMap(u32),
    ...u32(imports.length), ...imports.flatMap(metadataName),
    ...u32(1), ...metadataName("_pvm_start"), ...u32(0),
  ];
  const data = encoder.encode("worker\0missing\0");
  const bytes = [
    0, 0x61, 0x73, 0x6d, 1, 0, 0, 0,
    ...section(0, [...name("epoca.pvm.meta"), ...metadata]),
    ...section(1, [3, 0x60, 2, 0x7f, 0x7e, 1, 0x7f,
      0x60, 0, 1, 0x7f, 0x60, 1, 0x7e, 0]),
    ...section(3, [...leb(bodies.length), 0, 1, 2, ...stages.map(() => 1)]),
    ...section(4, [1, 0x70, 0, ...leb(stages.length)]),
    ...section(5, [1, 0, 1]),
    ...section(6, [globals.length, ...globals.flat()]),
    ...section(7, [exports.length, ...exports.flat()]),
    ...section(9, [1, 0, ...i32(0), 0x0b, ...leb(stages.length),
      ...stages.flatMap((_, index) => leb(index + 3))]),
    ...section(10, [...leb(bodies.length), ...bodies.flatMap((body) => [...leb(body.length), ...body])]),
    ...section(11, [1, 0, ...i32(0), 0x0b, ...leb(data.length), ...data]),
  ];
  return { module: new WebAssembly.Module(Uint8Array.from(bytes)), parts: [] };
}

function supervisor(module, options = null) {
  return new ComputerSupervisor(module, computerContext([], []), 1_000_000, null, options);
}

function drain(target, output) {
  for (let bytes; (bytes = target.takeTerminalOutput()) !== null;) output.push(...bytes);
}

function finish(target, output) {
  for (let turn = 0; turn < 32; turn++) {
    const status = target.run();
    drain(target, output);
    if (status.kind === "exited") return status.code;
    assert.equal(status.kind, "yielded");
  }
  assert.fail("finite guest did not finish within 32 host turns");
}

function requestLoop(count, method = "process_wait") {
  return guest(({ repeat, call, launch, report, exit }) => {
    repeat(count, () => {
      if (method === "process_wait") call(method, [999]);
      else launch(method, 7, 7);
      report();
    });
    exit();
  });
}

for (const [method, status] of [["process_wait", -2], ["process_run", -4], ["process_spawn", -4]]) {
  test(`${method} loop yields and resumes without replaying requests or output`, () => {
    const target = supervisor(requestLoop(REQUESTS, method));
    assert.deepEqual(target.run(), { kind: "yielded" });
    const output = [];
    drain(target, output);
    assert.deepEqual(output, Array(BUDGET - 1).fill(status & 255));
    assert.equal(finish(target, output), 23);
    assert.deepEqual(output, Array(REQUESTS).fill(status & 255));
    assert.deepEqual(target.run(), { kind: "exited", code: 23 });
  });
}

for (const count of [BUDGET - 1, BUDGET, BUDGET + 1]) {
  test(`foreground boundary with ${count} requests preserves the final resolved request`, () => {
    const target = supervisor(requestLoop(count));
    assert.deepEqual(target.run(), count < BUDGET ? { kind: "exited", code: 23 } : { kind: "yielded" });
    const output = [];
    drain(target, output);
    assert.equal(finish(target, output), 23);
    assert.deepEqual(output, Array(count).fill(254));
  });
}

for (const workspace of [false, true]) {
  test(`${workspace ? "workspace" : "piped"} children spend the shared budget and preserve output`, () => {
    const child = requestLoop(REQUESTS);
    const parent = guest(({ launch, saveHandle, repeat, call, forward, exit }) => {
      launch(workspace ? "workspace_spawn" : "process_spawn");
      saveHandle();
      repeat(32, () => {
        call(workspace ? "workspace_read" : "pipe_read", [get(0), BUFFER, 16384]);
        forward();
      });
      call(workspace ? "workspace_wait" : "process_wait", [get(0)]);
      exit(get(7));
    });
    const target = supervisor(parent);
    target.setWorkspaceEnabled(workspace);
    target.registerPackage("worker", child);
    assert.deepEqual(target.run(), { kind: "yielded" });
    const output = [];
    drain(target, output);
    assert.ok(output.length < REQUESTS);
    assert.equal(finish(target, output), 23);
    assert.deepEqual(output, Array(REQUESTS).fill(workspace ? 254 : 251));
  });
}

test("faulting foreground children remain contained beyond 32 faults and across turns", () => {
  const parent = guest(({ repeat, launch, report, exit }) => {
    repeat(5000, () => { launch("process_run"); report(); });
    exit();
  });
  const target = supervisor(parent);
  target.registerPackage("worker", guest(({ trap }) => trap()));
  assert.deepEqual(target.run(), { kind: "yielded" });
  const output = [];
  drain(target, output);
  assert.equal(finish(target, output), 23);
  assert.deepEqual(output, Array(5000).fill(139));
});

for (const workspace of [false, true]) {
  test(`queued input survives a ${workspace ? "workspace" : "pipe"} send at the budget boundary`, () => {
    const child = guest(({ call, forward, exit }) => {
      call("tty_read", [1, BUFFER, 32]);
      forward();
      exit();
    });
    const parent = guest(({ launch, saveHandle, repeat, call, report, forward, exit }) => {
      launch(workspace ? "workspace_spawn" : "process_spawn");
      saveHandle();
      repeat(BUDGET - 2, () => call("process_wait", [999]));
      call(workspace ? "workspace_send_input" : "pipe_write", [get(0), ADDRESS, 6]);
      report();
      call(workspace ? "workspace_read" : "pipe_read", [get(0), BUFFER, 32]);
      forward();
      call(workspace ? "workspace_wait" : "process_wait", [get(0)]);
      exit(get(7));
    });
    const target = supervisor(parent);
    target.setWorkspaceEnabled(workspace);
    target.registerPackage("worker", child);
    assert.deepEqual(target.run(), { kind: "yielded" });
    const output = [];
    drain(target, output);
    assert.deepEqual(output, []);
    assert.equal(finish(target, output), 23);
    assert.deepEqual(output, [6, ...encoder.encode("worker")]);
  });
}

for (const provide of [false, true]) {
  test(`budget yield surfaces a nested package request and ${provide ? "provides" : "rejects"} it once`, () => {
    const child = guest(({ launch, report, exit }) => {
      launch("process_run", 7, 7);
      report();
      exit();
    });
    const parent = guest(({ launch, saveHandle, call, repeat, forward, exit }) => {
      launch("workspace_spawn");
      saveHandle();
      repeat(BUDGET - 3, () => call("process_wait", [999]));
      call("workspace_wait", [get(0)]);
      repeat(REQUESTS, () => call("process_wait", [999]));
      call("workspace_read", [get(0), BUFFER, 32]);
      forward();
      call("workspace_wait", [get(0)]);
      exit(get(7));
    });
    const target = supervisor(parent, { packageResolution: true });
    target.setWorkspaceEnabled(true);
    target.registerPackage("worker", child);
    const pending = { kind: "package", package: "missing" };
    assert.deepEqual(target.run(), pending);
    assert.deepEqual(target.run(), pending);
    if (provide) target.providePackage(guest(({ exit }) => exit(42)));
    else target.rejectPackage();
    const output = [];
    assert.equal(finish(target, output), 23);
    assert.deepEqual(output, [provide ? 42 : 252]);
    assert.equal(target.pendingPackage(), null);
  });
}

test("a foreground child shares its parent's turn and resumes on the same stack", () => {
  const parent = guest(({ launch, report, exit }) => {
    launch("process_run");
    report();
    exit();
  });
  const target = supervisor(parent);
  target.registerPackage("worker", requestLoop(REQUESTS));
  assert.deepEqual(target.run(), { kind: "yielded" });
  const output = [];
  drain(target, output);
  assert.deepEqual(output, Array(BUDGET - 2).fill(254));
  assert.equal(finish(target, output), 23);
  assert.deepEqual(output, [...Array(REQUESTS).fill(254), 23]);
});
