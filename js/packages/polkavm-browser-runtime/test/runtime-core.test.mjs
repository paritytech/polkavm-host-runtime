import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

await import("../src/polkavm-wasm-translated.js");
await import("../src/polkavm-runtime-core.js");

const packageRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(packageRoot, "../../..");

function bytesBuffer(bytes) {
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  );
}

function partitionedGuestBytes({
  outOfGasOnBegin = false,
  meteredHostcalls = null,
} = {}) {
  const uleb = (value) => {
    const bytes = [];
    do {
      const byte = value & 0x7f;
      value >>>= 7;
      bytes.push(byte | (value ? 0x80 : 0));
    } while (value);
    return bytes;
  };
  const sleb = (value) => {
    const bytes = [];
    for (;;) {
      const byte = value & 0x7f;
      value >>= 7;
      const done = (value === 0 && !(byte & 0x40)) ||
        (value === -1 && (byte & 0x40));
      bytes.push(byte | (done ? 0 : 0x80));
      if (done) return bytes;
    }
  };
  const string = (value) => {
    const bytes = [...new TextEncoder().encode(value)];
    return [...uleb(bytes.length), ...bytes];
  };
  const section = (id, bytes) => [id, ...uleb(bytes.length), ...bytes];
  const vector = (entries) => [...uleb(entries.length), ...entries.flat()];
  const body = (instructions) => {
    const bytes = [0, ...instructions, 0x0b];
    return [...uleb(bytes.length), ...bytes];
  };
  const wasm = (...sections) =>
    new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, ...sections.flat()]);
  const exportEntry = (name, kind, index) => [...string(name), kind, index];
  const importEntry = (name, descriptor) => [
    ...string("pvm"),
    ...string(name),
    ...descriptor,
  ];
  const blockType = [0x60, 0, 1, 0x7f]; // () -> i32 status
  const part = (slot, instructions) =>
    wasm(
      section(1, vector([blockType])),
      section(
        2,
        vector([
          importEntry("__helper0", [0, 0]),
          importEntry("__table", [1, 0x70, 0, 2]),
          importEntry("memory", [2, 0, 1]),
          importEntry("__global0", [3, 0x7e, 1]),
        ]),
      ),
      section(3, [1, 0]),
      section(9, [1, 0, 0x41, slot, 0x0b, 1, 1]),
      section(10, vector([body(instructions)])),
    );
  const first = part(0, [
    0x23, 0, 0x42, 7, 0x7c, 0x24, 0, // r0 += 7
    0x41, 0, 0x41, 0, 0x28, 2, 0, // address 0, memory[0]
    0x41, 5, 0x6a, 0x36, 2, 0, // memory[0] += 5
    0x41, 1, 0x13, 0, 0, // tail-dispatch to part 1 through the shared table
  ]);
  const second = part(1, [
    0x41, 4, 0x10, 0, // address 4, root helper reading memory[0]
    0x23, 0, 0xa7, 0x6a, 0x36, 2, 0, // memory[4] = memory[0] + r0
    0x41, 0x7f, // STATUS_FINISHED
  ]);
  const metadataExport = (name) => [
    name.length, 0, ...new TextEncoder().encode(name), 0, 0, 0, 0,
  ];
  const metadataString = (name) => [
    name.length, 0, ...new TextEncoder().encode(name),
  ];
  const metadata = [
    ...new TextEncoder().encode("EPM2"),
    1, 0, 0, 0, // 64-bit registers
    ...new Array(9 * 4).fill(0),
    0, 0, 1, 0, // stackHigh = 65536, identity-mapped memory
    0, 0, 0, 0, // stackPhysical
    ...(meteredHostcalls
      ? [
        meteredHostcalls.frames ? 2 : 1, 0, 0, 0,
        ...metadataString("host_update_after"),
        ...(meteredHostcalls.frames ? metadataString("pvm_display") : []),
        meteredHostcalls.entry === "init" ? 2 : 1, 0, 0, 0,
        ...(meteredHostcalls.entry === "init" ? metadataExport("init") : []),
        ...metadataExport(meteredHostcalls.frames ? "_pvm_start" : "update"),
      ]
      : [
        0, 0, 0, 0, // imports
        2, 0, 0, 0, // exports
        ...metadataExport("init"),
        ...metadataExport("update"),
      ]),
  ];
  // A real Wasm dispatcher: each ECALL costs gas, resumes at the next call,
  // and leaves its remaining gas in the same exported global as the compiler.
  const meteredResume = meteredHostcalls ? [
    0x23, 14, 0x41, ...sleb(meteredHostcalls.count), 0x4f, // calls >= count
    0x04, 0x40, 0x41, 0x7f, 0x0f, 0x0b, // return FINISHED
    0x23, 13, 0x42, ...sleb(meteredHostcalls.cost), 0x53, // gas < cost
    0x04, 0x40, 0x41, 0x7c, 0x0f, 0x0b, // return OUT_OF_GAS
    0x23, 13, 0x42, ...sleb(meteredHostcalls.cost), 0x7d, 0x24, 13,
    ...(meteredHostcalls.idle
      ? [0x42, 0x7f, 0x24, 7] // idle
      : [0x42, 23, 0x42, 50, 0x23, 14, 0x45, 0x1b, 0x24, 7]), // first delay 23, then 50
    ...(meteredHostcalls.frames ? [
      0x23, 14, 0x41, 1, 0x71, 0x24, 15, // odd calls display, even calls schedule
      0x23, 15, 0x04, 0x40,
      0x42, 1, 0x24, 7, 0x42, 1, 0x24, 8, // 1x1 frame at address 0
      0x0b,
    ] : []),
    0x23, 14, 0x41, 1, 0x6a, 0x24, 14, // calls++
    0x41, 0x7e, // ECALL
  ] : null;
  const rootSections = [
    section(0, [...string("epoca.pvm.meta"), ...metadata]),
    section(
      1,
      vector([
        blockType,
        [0x60, 2, 0x7f, 0x7e, 1, 0x7f], // begin(i32, i64) -> i32
        [0x60, 1, 0x7e, 0], // set_gas(i64)
      ]),
    ),
    section(3, outOfGasOnBegin || meteredHostcalls
      ? [4, 0, 1, 2, 0] : [3, 0, 1, 2]),
    section(4, [1, 0x70, 0, 2]),
    section(5, [1, 0, 1]),
    section(
      6,
      vector([
        ...Array.from({ length: 14 }, () => [0x7e, 1, 0x42, 0, 0x0b]),
        [0x7f, 1, 0x41, 0, 0x0b], // completed hostcalls
        [0x7f, 1, 0x41, 0, 0x0b], // ecall import index
      ]),
    ),
    section(
      7,
      vector([
        exportEntry("memory", 2, 0),
        exportEntry("__table", 1, 0),
        exportEntry("__global0", 3, 0),
        exportEntry("__helper0", 0, 0),
        exportEntry("pvm_begin", 0, 1),
        exportEntry("pvm_set_gas", 0, 2),
        ...(outOfGasOnBegin || meteredHostcalls
          ? [exportEntry("pvm_resume", 0, 3)] : []),
        exportEntry("gas", 3, 13),
        exportEntry("calls", 3, 14),
        exportEntry("ecall", 3, 15),
        ...Array.from({ length: 13 }, (_, index) =>
          exportEntry(`r${index}`, 3, index),
        ),
      ]),
    ),
    section(
      10,
      vector([
        body([0x41, 0, 0x28, 2, 0]), // helper reads memory[0]
        body(meteredHostcalls
          ? [
            0x20, 1, 0x24, 13, // gas = begin argument
            0x41, 0, 0x24, 14, // calls = 0
            0x12, 3, // tail-call metered dispatcher
          ]
          : outOfGasOnBegin
            ? [0x41, 0x7c] // STATUS_OUT_OF_GAS
            : [0x20, 1, 0x24, 13, 0x20, 0, 0x13, 0, 0]),
        body([0x20, 0, 0x24, 13]),
        ...(meteredHostcalls
          ? [body(meteredResume)]
          : outOfGasOnBegin ? [body([0x41, 0, 0x13, 0, 0])] : []),
      ]),
    ),
  ];
  return {
    root: wasm(...rootSections),
    partitioned: wasm(
      ...rootSections,
      section(0, [...string("epoca.pvm.code-part"), ...first]),
      section(0, [...string("epoca.pvm.code-part"), ...second]),
    ),
    invalidPart: wasm(
      ...rootSections,
      section(0, [...string("epoca.pvm.code-part"), 0]),
    ),
  };
}

test("translated code parts share guest memory, registers, helpers and control flow", async () => {
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const { partitioned } = partitionedGuestBytes();
  const program = structuredClone(await Runtime.compile(partitioned));
  assert.equal(Runtime.isCompiledProgram(program), true);
  const translated = new Runtime(
    program, [], () => {}, 1_000_000, false, "framebuffer",
  );
  const state = () => ({
    register: translated.pvm.r0.value,
    memory: [...new Uint32Array(translated.memory.buffer, 0, 2)],
  });
  translated.initialize();
  assert.deepEqual(state(), { register: 7n, memory: [5, 12] });
  translated.update(17);
  assert.deepEqual(state(), { register: 14n, memory: [10, 24] });
  const other = new Runtime(
    program, [], () => {}, 1_000_000, false, "framebuffer",
  );
  other.initialize();
  assert.deepEqual(
    [...new Uint32Array(other.memory.buffer, 0, 2)],
    [5, 12],
    "cached code must not share guest state between runtime instances",
  );
  assert.deepEqual(state(), { register: 14n, memory: [10, 24] });
  translated.stop();
  other.stop();
});

test("translated execution resumes after exhausting a gas slice", async () => {
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const { partitioned } = partitionedGuestBytes({ outOfGasOnBegin: true });
  const program = await Runtime.compile(partitioned);
  const translated = new Runtime(
    program, [], () => {}, 1_000_000, false, "framebuffer", null, 0, [], null,
    2,
  );
  const state = () => ({
    register: translated.pvm.r0.value,
    memory: [...new Uint32Array(translated.memory.buffer, 0, 2)],
  });

  translated.initialize();
  assert.equal(translated.hasPendingContinuation(), true);
  assert.deepEqual(state(), { register: 0n, memory: [0, 0] });

  translated.update(1);
  assert.equal(translated.hasPendingContinuation(), false);
  assert.deepEqual(state(), { register: 7n, memory: [5, 12] });

  translated.update(2);
  assert.equal(translated.hasPendingContinuation(), true);
  translated.update(3);
  assert.equal(translated.hasPendingContinuation(), false);
  assert.deepEqual(state(), { register: 14n, memory: [10, 24] });
  translated.stop();

  const bounded = new Runtime(
    program, [], () => {}, 1_000_000, false, "framebuffer",
  );
  assert.throws(
    () => bounded.initialize(),
    /translated PolkaVM guest ran out of gas/,
  );
  bounded.stop();
});

async function meteredRuntime({ count, cost, entry, frames, gas, slices = 1 }) {
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const { root } = partitionedGuestBytes({
    meteredHostcalls: { count, cost, entry, frames },
  });
  return new Runtime(
    await Runtime.compile(root), [], () => {}, gas, false, "framebuffer",
    null, 0, [], null, slices,
  );
}

test("translated scheduling requests and call bounds survive gas continuations", async () => {
  const runtime = await meteredRuntime({
    count: 2, cost: 100, gas: 100, slices: 2,
  });
  runtime.initialize();
  runtime.update(1);
  assert.equal(runtime.hasPendingContinuation(), true);
  assert.equal(runtime.updateAfterMilliseconds(), 23);
  // These bounds belong to the logical call, not the worker's scheduling tick.
  runtime.gpuSubmits = 1;
  runtime.hostFrameRequests = 2;
  runtime.hostFrameRequestBytes = 3;
  runtime.uiSemanticsSubmitted = true;
  runtime.uiOutputSubmitted = true;
  runtime.tri2dSubmitted = true;
  runtime.mediatedInputCommands = 4;
  runtime.hostcallBytes = 5;
  runtime.update(2);
  assert.equal(runtime.hasPendingContinuation(), false);
  assert.equal(runtime.updateAfterMilliseconds(), 23, "retain the earliest request");
  const callBounds = () => [
    runtime.gpuSubmits, runtime.hostFrameRequests, runtime.hostFrameRequestBytes,
    runtime.uiSemanticsSubmitted, runtime.uiOutputSubmitted, runtime.tri2dSubmitted,
    runtime.mediatedInputCommands, runtime.hostcallBytes,
  ];
  assert.deepEqual(callBounds(), [1, 2, 3, true, true, true, 4, 5]);
  runtime.update(3);
  assert.deepEqual(callBounds(), [0, 0, 0, false, false, false, 0, 32 * 1024 * 1024]);
  assert.equal(runtime.pvm.calls.value, 1, "the next update starts a new call");
  runtime.stop();
  runtime.update(4);
  assert.equal(runtime.pvm.calls.value, 1, "stop must not resume a pending call");
});

test("translated hostcall yields cannot refill the complete-call gas budget", async () => {
  const runtime = await meteredRuntime({
    count: 4 * 65536 - 1, cost: 100, gas: 10_000_000, slices: 2,
  });
  runtime.initialize();
  runtime.update(1);
  assert.equal(runtime.pvm.calls.value, 65536);
  assert.equal(runtime.pvm.gas.value, 3_446_400n);
  runtime.update(2);
  assert.equal(runtime.pvm.calls.value, 100000, "exhaust the first gas quantum");
  runtime.update(3);
  assert.equal(runtime.pvm.calls.value, 165536);
  assert.throws(() => runtime.update(4), /guest ran out of gas/);
  assert.equal(runtime.pvm.calls.value, 200000, "at most two gas quanta are spent");
  runtime.stop();
});

test("translated single-slice calls retain unused gas through hostcall yields", async () => {
  const runtime = await meteredRuntime({
    count: 65537, cost: 100, gas: 10_000_000,
  });
  runtime.initialize();
  runtime.update(1);
  assert.equal(runtime.hasPendingContinuation(), true);
  runtime.update(2);
  assert.equal(runtime.hasPendingContinuation(), false);
  assert.equal(runtime.pvm.gas.value, 3_446_300n);
  assert.equal(runtime.updateAfterMilliseconds(), 23);
  runtime.stop();
});

test("translated initialization continues without losing its remaining gas", async () => {
  const runtime = await meteredRuntime({
    entry: "init", count: 150000, cost: 100, gas: 10_000_000, slices: 2,
  });
  runtime.initialize();
  assert.equal(runtime.pvm.calls.value, 100000);
  runtime.update(1);
  assert.equal(runtime.hasPendingContinuation(), false);
  assert.equal(runtime.pvm.calls.value, 150000);
  assert.equal(runtime.pvm.gas.value, 5_000_000n);
  runtime.stop();
});

test("translated reduced initialization budgets survive hostcall yields", async () => {
  const runtime = await meteredRuntime({
    entry: "init", count: 1024 * 1024 + 2, cost: 1, gas: 2_000_000, slices: 4,
  });
  runtime.initialize(1024 * 1024 + 1);
  assert.equal(runtime.hasPendingContinuation(), true);
  assert.equal(runtime.pvm.gas.value, 1n);
  assert.throws(() => runtime.update(1), /guest ran out of gas/);
  assert.equal(runtime.pvm.calls.value, 1024 * 1024 + 1);
  runtime.stop();
});

test("translated CoreVM frame boundaries start fresh scheduling and gas budgets", async () => {
  const runtime = await meteredRuntime({
    frames: true, count: 4, cost: 100, gas: 200,
  });
  runtime.initialize();
  runtime.update(1);
  assert.equal(runtime.pvm.calls.value, 2);
  assert.equal(runtime.pvm.gas.value, 0n);
  assert.equal(runtime.hasPendingContinuation(), false, "a frame is not a gas continuation");
  assert.equal(runtime.updateAfterMilliseconds(), 23);
  runtime.update(2);
  assert.equal(runtime.pvm.calls.value, 4, "resume rather than restarting _pvm_start");
  assert.equal(runtime.pvm.gas.value, 0n, "the next frame gets a fresh gas budget");
  assert.equal(runtime.updateAfterMilliseconds(), 50, "the old frame's request is cleared");
  runtime.stop();
});

test("compiled programs accept root-only modules but reject malformed parts and bare modules", async () => {
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const { root } = partitionedGuestBytes();
  const program = await Runtime.compile(root);
  assert.equal(Runtime.isCompiledProgram(program), true);
  const translated = new Runtime(
    program, [], () => {}, 1_000_000, false, "framebuffer",
  );
  translated.stop();
  for (const invalid of [
    program.module,
    { module: program.module },
    { module: program.module, parts: [null] },
    { module: program.module, parts: new Array(1) },
    { module: root, parts: [] },
  ]) {
    assert.equal(Runtime.isCompiledProgram(invalid), false);
    assert.throws(
      () => new Runtime(invalid, [], () => {}, 1_000_000, false, "framebuffer"),
      TypeError,
    );
  }
});

function endpoint() {
  const messages = [];
  const receiver = {
    onmessage: null,
    postMessage(message) {
      messages.push(message);
    },
  };
  globalThis.createPolkaVmRuntime(receiver);
  return { messages, receiver };
}

function controlledTicks(t) {
  const ticks = [];
  const timers = new Map();
  let nextTimer = 0;
  t.mock.method(globalThis, "MessageChannel", class {
    constructor() {
      this.port1 = { onmessage: null, close() { this.onmessage = null; } };
      this.port2 = {
        postMessage: () => ticks.push(() => this.port1.onmessage?.()),
        close() {},
      };
    }
  });
  return {
    ticks,
    timers,
    controlTimers(dispatch = (callback) => callback()) {
      // Install after asynchronous Wasm startup; waitForMessage uses timers.
      t.mock.method(globalThis, "setTimeout", (callback, delay = 0) => {
        const id = ++nextTimer;
        timers.set(id, () => dispatch(callback, delay));
        return id;
      });
      t.mock.method(globalThis, "clearTimeout", (id) => timers.delete(id));
    },
    drain() {
      let count = 0;
      while (ticks.length) {
        assert.ok(count++ < 100, "background work must become idle");
        ticks.shift()();
      }
      return count;
    },
  };
}

function hostResponseEchoGuest(demandDriven = false, clockCalls = 0) {
  // Tiny real Latest32 PolkaVM guest: init returns; each update polls one byte
  // into rw_data (0x20000), then saves it. Distinct response bytes make ordering
  // and loss observable through the same hostcalls on both execution backends.
  const text = (value) => [...new TextEncoder().encode(value)];
  const section = (id, bytes) => [id, bytes.length, ...bytes];
  const imports = ["host_frame_poll", "host_save_submit"];
  if (demandDriven) {
    imports.push("host_update_after");
  }
  const clockImport = imports.length;
  if (clockCalls) {
    imports.push("host_time_ms");
  }
  const symbols = [];
  const offsets = [];
  for (const name of imports) {
    offsets.push(symbols.length, 0, 0, 0);
    symbols.push(...text(name));
  }
  const instructions = [
    [50, 0], // init: ret
    ...Array.from({ length: clockCalls }, () => [10, clockImport]),
    [51, 7, 0, 0, 2], // update: a0 = 0x20000
    [51, 8, 1], // a1 = capacity 1
    [10], // host_frame_poll
    [51, 7, 0, 0, 2], // a0 = saved response address
    [51, 8, 1], // a1 = length 1
    [10, 1], // host_save_submit
    ...(demandDriven ? [[51, 7, 255], [10, 2]] : []), // idle
    [50, 0], // ret
  ];
  const code = instructions.flat();
  const bitmask = new Uint8Array(Math.ceil(code.length / 8));
  let offset = 0;
  for (const instruction of instructions) {
    bitmask[offset >> 3] |= 1 << (offset & 7);
    offset += instruction.length;
  }
  const bytes = new Uint8Array([
    ...text("PVM\0"), 1, ...new Array(8).fill(0),
    ...section(1, [0, 1, 0]), // ro_data, rw_data, stack sizes
    ...section(4, [imports.length, ...offsets, ...symbols]),
    ...section(5, [2, 0, 4, ...text("init"), 2, 6, ...text("update")]),
    ...section(6, [0, 0, code.length, ...code, ...bitmask]),
    0,
  ]);
  new DataView(bytes.buffer).setBigUint64(5, BigInt(bytes.length), true);
  return bytes;
}

async function settle() {
  await new Promise((resolve) => setImmediate(resolve));
}

async function waitForMessage(messages, type, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const message = messages.find((candidate) => candidate.type === type);
    if (message) {
      return message;
    }
    const error = messages.find((candidate) => candidate.type === "error");
    if (error) {
      throw new Error(error.message);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timed out waiting for browser runtime message ${type}`);
}
async function waitForStartupStage(messages, stage, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (
      messages.some(
        (candidate) =>
          candidate.type === "startup" && candidate.stage === stage,
      )
    ) {
      return;
    }
    const error = messages.find((candidate) => candidate.type === "error");
    if (error) {
      throw new Error(error.message);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(
    `timed out waiting for browser runtime startup stage ${stage}`,
  );
}

function invalidStart(overrides = {}) {
  return {
    type: "start",
    runtime: new Uint8Array([0]),
    program: new Uint8Array([1]),
    assets: [],
    graphicsProfile: "framebuffer",
    audioEnabled: false,
    cacheKey: "invalid",
    ...overrides,
  };
}

function pointerDelta(x, y) {
  const bytes = new Uint8Array(8);
  const view = new DataView(bytes.buffer);
  bytes[0] = 6;
  view.setInt16(2, x, true);
  view.setInt16(4, y, true);
  return bytes;
}

function motionSample() {
  const bytes = new Uint8Array(48);
  const view = new DataView(bytes.buffer);
  bytes.set([0x50, 0x4d, 0x4f, 0x31]);
  view.setUint16(4, 1, true);
  view.setUint16(6, 6, true);
  view.setUint32(8, 48, true);
  view.setUint32(12, 1, true);
  view.setFloat64(16, 10, true);
  view.setFloat32(40, -2, true);
  view.setFloat32(44, 4, true);
  return bytes;
}

function motionResult(bytes) {
  const value = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  return {
    status: new DataView(
      value.buffer,
      value.byteOffset,
      value.byteLength,
    ).getInt32(0, true),
    sample: value.subarray(4),
  };
}

function gpuCapabilities(surfaceGeneration) {
  const bytes = new Uint8Array(56);
  const view = new DataView(bytes.buffer);
  bytes.set([0x45, 0x47, 0x43, 0x31]);
  view.setUint16(4, 1, true);
  view.setUint32(8, bytes.byteLength, true);
  view.setUint16(12, 1, true);
  view.setUint32(16, 640, true);
  view.setUint32(20, 480, true);
  view.setUint32(24, 640, true);
  view.setUint32(28, 480, true);
  view.setFloat32(32, 1, true);
  view.setUint32(36, surfaceGeneration, true);
  view.setUint32(40, 1, true);
  return bytes;
}

test("browser runtime rejects unbounded launch inputs before compilation", async () => {
  for (const [message, expected] of [
    [invalidStart({ program: new Uint8Array() }), /program must contain/],
    [
      invalidStart({
        assets: [
          { path: "same.bin", bytes: new Uint8Array() },
          { path: "same.bin", bytes: new Uint8Array() },
        ],
      }),
      /duplicated/,
    ],
    [
      invalidStart({
        assets: [{ path: "../escape", bytes: new Uint8Array() }],
      }),
      /invalid PolkaVM browser asset path/,
    ],
    [
      invalidStart({ graphicsProfile: "webgpu-raster" }),
      /WebGPU capabilities are required/,
    ],
    [
      invalidStart({ motionAvailability: 3 }),
      /invalid PolkaVM browser motion availability/,
    ],
    [
      invalidStart({ mediatedInputKinds: ["Camera UR"] }),
      /invalid PolkaVM browser mediated-input kinds/,
    ],
  ]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({ data: message });
    await settle();
    assert.match(
      messages.find((candidate) => candidate.type === "error")?.message ?? "",
      expected,
    );
  }
});

test("continuous guests hold 60 Hz despite timer dispatch latency", async (t) => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const scheduler = controlledTicks(t);
  const originalRuntime = globalThis.TranslatedPolkaVmRuntime;
  let now = 0;
  t.mock.method(performance, "now", () => now);
  const updateStartedAt = [];
  globalThis.TranslatedPolkaVmRuntime = class extends originalRuntime {
    update(timeMs) {
      updateStartedAt.push(performance.now());
      super.update(timeMs);
    }
  };
  const { messages, receiver } = endpoint();
  try {
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: "continuous-update-cadence",
      },
    });
    await waitForMessage(messages, "ready");
    scheduler.controlTimers((callback, delay) => {
      now += delay + 3;
      callback();
    });
    for (let update = 0; update < 31; update++) {
      assert.equal(scheduler.ticks.length, 1, "continuous updates stalled");
      scheduler.drain();
      if (update === 30) break;
      assert.equal(scheduler.timers.size, 1);
      const [id, callback] = scheduler.timers.entries().next().value;
      scheduler.timers.delete(id);
      callback();
    }
    const elapsed = updateStartedAt[30] - updateStartedAt[0];
    assert.ok(
      Math.abs(elapsed - 503) < 1e-6,
      `thirty frame intervals must accumulate only one 3 ms dispatch delay: ${elapsed}`,
    );
  } finally {
    receiver.onmessage({ data: { type: "stop" } });
    globalThis.TranslatedPolkaVmRuntime = originalRuntime;
    t.mock.restoreAll();
    await waitForMessage(messages, "terminated");
  }
});

test("demand-driven guests idle until an external event wakes them", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const originalRuntime = globalThis.TranslatedPolkaVmRuntime;
  // Exercise scheduling independently of the guest's import declaration, while
  // retaining real translation, guest execution and framebuffer output.
  globalThis.TranslatedPolkaVmRuntime = class extends originalRuntime {
    usesUpdateScheduling() {
      return true;
    }
  };
  const { messages, receiver } = endpoint();
  try {
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: "demand-driven-idle",
      },
    });
    const ready = await waitForMessage(messages, "ready");
    assert.equal(ready.backend, "compiler");
    await waitForMessage(messages, "frame");
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.equal(messages.filter((message) => message.type === "frame").length, 1);

    messages.length = 0;
    receiver.onmessage({ data: { type: "input", bytes: new Uint8Array(8) } });
    await waitForMessage(messages, "frame");
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.equal(messages.filter((message) => message.type === "frame").length, 1);
  } finally {
    receiver.onmessage({ data: { type: "stop" } });
    globalThis.TranslatedPolkaVmRuntime = originalRuntime;
    await waitForMessage(messages, "terminated");
  }
});

test("demand-driven delays start after the completed update", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const originalRuntime = globalThis.TranslatedPolkaVmRuntime;
  const updateStartedAt = [];
  globalThis.TranslatedPolkaVmRuntime = class extends originalRuntime {
    update(timeMs) {
      const startedAt = performance.now();
      updateStartedAt.push(startedAt);
      super.update(timeMs);
      if (updateStartedAt.length === 1) {
        while (performance.now() - startedAt < 25) {}
      }
    }
    usesUpdateScheduling() {
      return true;
    }
    updateAfterMilliseconds() {
      return updateStartedAt.length === 1 ? 40 : null;
    }
  };
  const { messages, receiver } = endpoint();
  try {
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: "demand-driven-delay-origin",
      },
    });
    while (updateStartedAt.length < 2) {
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    assert.ok(
      updateStartedAt[1] - updateStartedAt[0] >= 55,
      "the requested delay must not overlap the preceding update",
    );
  } finally {
    receiver.onmessage({ data: { type: "stop" } });
    globalThis.TranslatedPolkaVmRuntime = originalRuntime;
    await waitForMessage(messages, "terminated");
  }
});

test("compiler backend enforces the declared graphics profile", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "tri2d",
      audioEnabled: false,
      cacheKey: "profile-enforcement",
    },
  });
  const ready = await waitForMessage(messages, "ready");
  assert.equal(ready.backend, "compiler");
  assert.equal(ready.usesMotion, false);
  await new Promise((resolve) => setTimeout(resolve, 100));
  assert.equal(
    messages.some((message) => message.type === "frame"),
    false,
    "a framebuffer submission must not escape a tri2d declaration",
  );
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("compiler backend returns complete u64 clock values to 32-bit guests", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/clock-u64.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "clock-u64",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const outputs = [];
  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    (output) => outputs.push(output),
    1_000_000,
    false,
    "framebuffer",
  );
  translated.initialize();

  translated.update(17);
  assert.deepEqual(
    outputs.findLast((output) => output.type === "save").bytes,
    new Uint8Array([17, 0, 0, 0, 0, 0, 0, 0]),
  );

  translated.update(0x2_0000_0011);
  assert.deepEqual(
    outputs.findLast((output) => output.type === "save").bytes,
    new Uint8Array([17, 0, 0, 0, 2, 0, 0, 0]),
  );
  translated.stop();
});

test("both browser backends freeze time across startup pause and queued ticks", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const program = await readFile(resolve(
    repositoryRoot, "rust/crates/polkavm-host-runtime/tests/fixtures/clock-u64.polkavm",
  ));
  for (const forceInterpreter of [false, true]) {
    await t.test(forceInterpreter ? "interpreter" : "compiler", async (t) => {
      let now = 100;
      const ticks = [];
      const timers = new Map();
      let nextTimer = 0;
      t.mock.method(performance, "now", () => now);
      t.mock.method(globalThis, "MessageChannel", class {
        constructor() {
          this.port1 = { onmessage: null, close() { this.onmessage = null; } };
          this.port2 = {
            postMessage: () => ticks.push(() => this.port1.onmessage?.()),
            close() {},
          };
        }
      });
      const { messages, receiver } = endpoint();
      const send = (data) => receiver.onmessage({ data });
      try {
        send({ type: "pause", paused: true });
        send({
          type: "start", runtime: bytesBuffer(runtime), program: bytesBuffer(program),
          assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
          cacheKey: `paused-clock-${forceInterpreter}`, forceInterpreter,
        });
        const ready = await waitForMessage(messages, "ready");
        assert.equal(ready.backend, forceInterpreter ? "interpreter" : "compiler");
        assert.equal(ticks.length, 0, "startup must not run the paused first update");
        t.mock.method(globalThis, "setTimeout", (callback) => {
          const id = ++nextTimer;
          timers.set(id, callback);
          return id;
        });
        t.mock.method(globalThis, "clearTimeout", (id) => timers.delete(id));
        const guestTime = () => {
          const bytes = messages.findLast((message) => message.type === "save").bytes;
          return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getBigUint64(0, true);
        };
        now = 10_000;
        send({ type: "pause", paused: false });
        assert.equal(ticks.length, 1);
        ticks.shift()();
        assert.equal(guestTime(), 0n, "startup pause is not guest elapsed time");
        assert.equal(timers.size, 1);

        now = 10_017;
        const [timerId, callback] = timers.entries().next().value;
        timers.delete(timerId);
        callback();
        assert.equal(ticks.length, 1);
        send({ type: "pause", paused: true });
        const savesBeforePause = messages.filter((message) => message.type === "save").length;
        ticks.shift()();
        assert.equal(messages.filter((message) => message.type === "save").length, savesBeforePause);
        assert.equal(timers.size, 0);
        now = 110_017;
        send({ type: "input", bytes: new Uint8Array([1, 4, 0, 0, 0, 0, 0, 0]) });
        send({ type: "pause", paused: true });
        assert.equal(ticks.length, 0, "paused input cannot wake or spin");
        send({ type: "pause", paused: false });
        send({ type: "pause", paused: false });
        assert.equal(ticks.length, 1, "resume and repeated acknowledgments cannot queue a burst");
        ticks.shift()();
        assert.equal(guestTime(), 17n, "resume excludes all paused wall time");
        assert.equal(timers.size, 1, "resume returns to paced updates");
        assert.equal(ticks.length, 0);
        assert.deepEqual(messages.findLast((message) => message.type === "pause-state"), {
          type: "pause-state", paused: false,
        });
      } finally {
        receiver.onmessage?.({ data: { type: "stop" } });
      }
    });
  }
});

test("background servicing preserves ordered bursts and retry backpressure without periodic work", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  for (const forceInterpreter of [false, true]) {
    for (const demandDriven of [false, true]) {
      await t.test(`${forceInterpreter ? "interpreter" : "compiler"} ${demandDriven ? "demand" : "legacy"}`, async (t) => {
        const scheduling = controlledTicks(t);
        const { messages, receiver } = endpoint();
        const send = (data) => receiver.onmessage({ data });
        try {
          send({
            type: "start", runtime: bytesBuffer(runtime),
            program: bytesBuffer(hostResponseEchoGuest(demandDriven)),
            assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
            cacheKey: `background-echo-${forceInterpreter}-${demandDriven}`, forceInterpreter,
          });
          const ready = await waitForMessage(messages, "ready");
          assert.equal(ready.backend, forceInterpreter ? "interpreter" : "compiler");
          assert.equal(ready.usesUpdateScheduling, demandDriven);
          scheduling.controlTimers();
          // These responses and the already posted first tick precede the
          // background transition; neither may be forgotten at that boundary.
          for (let seq = 0; seq < 33; seq++) {
            send({ type: "host-frame-response", seq, bytes: new Uint8Array([seq + 1]) });
          }
          assert.deepEqual(messages.findLast((message) => message.type === "host-frame-response-rejected"), {
            type: "host-frame-response-rejected", seq: 32, reason: "queue-full",
          });
          send({ type: "background", backgrounded: true, seq: 7 });
          assert.equal(scheduling.ticks.length, 1, "bursts share one pending MessageChannel task");
          assert.equal(scheduling.drain(), 32);
          assert.deepEqual(messages.filter((message) => message.type === "save").map((message) => message.bytes[0]),
            Array.from({ length: 32 }, (_, index) => index + 1));
          assert.equal(scheduling.timers.size, 0, "legacy cadence and requested delays cannot spin in background");
          for (let seq = 32; seq < 70; seq++) {
            send({ type: "host-frame-response", seq, bytes: new Uint8Array([seq + 1]) });
            assert.deepEqual(messages.at(-1), { type: "host-frame-response-accepted", seq });
            assert.equal(scheduling.drain(), 1, "one response buys one service update, not a full burst");
          }
          assert.deepEqual(messages.filter((message) => message.type === "save").map((message) => message.bytes[0]),
            Array.from({ length: 70 }, (_, index) => index + 1));
          send({ type: "input", bytes: new Uint8Array([1, 4, 0, 0, 0, 0, 0, 0]) });
          send({ type: "motion", bytes: motionSample() });
          send({ type: "background", backgrounded: true, seq: 8 });
          assert.equal(scheduling.ticks.length, 0);
          assert.equal(scheduling.timers.size, 0);
          send({ type: "pause", paused: true });
          send({ type: "host-frame-response", seq: 70, bytes: new Uint8Array([71]) });
          assert.equal(scheduling.ticks.length, 0, "hard pause takes precedence over response wakes");
          send({ type: "pause", paused: false });
          assert.equal(scheduling.drain(), 1);
          assert.equal(messages.findLast((message) => message.type === "save").bytes[0], 71);
          send({ type: "background", backgrounded: false, seq: 9 });
          assert.deepEqual(messages.at(-1), { type: "background-state", backgrounded: false, seq: 9 });
          scheduling.ticks.shift()();
          assert.equal(scheduling.timers.size, demandDriven ? 0 : 1, "foreground cadence returns");
        } finally {
          receiver.onmessage?.({ data: { type: "stop" } });
        }
      });
    }
  }
});

test("background and hard pause freeze their combined interval across startup and queued ticks", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const program = await readFile(resolve(repositoryRoot,
    "rust/crates/polkavm-host-runtime/tests/fixtures/clock-u64.polkavm"));
  for (const forceInterpreter of [false, true]) {
    await t.test(forceInterpreter ? "interpreter" : "compiler", async (t) => {
      let now = 100;
      t.mock.method(performance, "now", () => now);
      const scheduling = controlledTicks(t);
      const { messages, receiver } = endpoint();
      const send = (data) => receiver.onmessage({ data });
      const guestTimes = () => messages.filter((message) => message.type === "save").map(({ bytes }) =>
        new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getBigUint64(0, true));
      try {
        send({ type: "background", backgrounded: true });
        send({
          type: "start", runtime: bytesBuffer(runtime), program: bytesBuffer(program),
          assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
          cacheKey: `background-clock-${forceInterpreter}`, forceInterpreter,
        });
        // This transition happens while asynchronous startup is in progress.
        send({ type: "pause", paused: true });
        await waitForMessage(messages, "ready");
        scheduling.controlTimers();
        assert.equal(scheduling.ticks.length, 0);
        now = 10_000;
        send({ type: "host-frame-response", bytes: new Uint8Array([1]) });
        send({ type: "pause", paused: false });
        scheduling.drain();
        assert.deepEqual(guestTimes(), [0n], "background startup time is frozen during service");
        now = 20_000;
        send({ type: "pause", paused: true });
        send({ type: "background", backgrounded: false });
        send({ type: "host-frame-response", bytes: new Uint8Array([2]) });
        assert.equal(scheduling.ticks.length, 0, "leaving background does not bypass hard pause");
        now = 30_000;
        send({ type: "pause", paused: false });
        scheduling.ticks.shift()();
        assert.deepEqual(guestTimes(), [0n, 0n], "overlapping inactivity is excluded exactly once");
        now = 30_017;
        const [id, callback] = scheduling.timers.entries().next().value;
        scheduling.timers.delete(id);
        callback();
        send({ type: "background", backgrounded: true });
        scheduling.drain();
        // This clock guest deliberately never polls its responses. Foreground
        // ticks must not erase the actual queued work at the next transition.
        assert.deepEqual(guestTimes(), [0n, 0n, 17n, 17n]);
        assert.equal(scheduling.ticks.length, 0, "unpolled work gets bounded opportunities, not a spin");
        assert.equal(scheduling.timers.size, 0);
        now = 130_017;
        send({ type: "host-frame-response", bytes: new Uint8Array([3]) });
        scheduling.drain();
        assert.deepEqual(guestTimes(), [0n, 0n, 17n, 17n, 17n]);
        now = 230_017;
        send({ type: "background", backgrounded: false });
        scheduling.ticks.shift()();
        assert.deepEqual(guestTimes(), [0n, 0n, 17n, 17n, 17n, 17n]);
      } finally {
        receiver.onmessage?.({ data: { type: "stop" } });
      }
    });
  }
});

test("background framebuffer retention resumes once after acknowledgment and never after stop", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const program = await readFile(resolve(repositoryRoot,
    "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm"));
  for (const forceInterpreter of [false, true]) {
    await t.test(forceInterpreter ? "interpreter" : "compiler", async (t) => {
      const scheduling = controlledTicks(t);
      const { messages, receiver } = endpoint();
      const send = (data) => receiver.onmessage({ data });
      try {
        send({ type: "background", backgrounded: true, seq: 1 });
        send({
          type: "start", runtime: bytesBuffer(runtime), program: bytesBuffer(program),
          assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
          cacheKey: `background-frame-${forceInterpreter}`, forceInterpreter,
        });
        await waitForMessage(messages, "ready");
        scheduling.controlTimers();
        for (let seq = 0; seq < 3; seq++) {
          send({ type: "host-frame-response", seq, bytes: new Uint8Array([seq]) });
          scheduling.drain();
        }
        assert.equal(messages.some((message) => message.type === "frame"), false);
        const start = messages.length;
        send({ type: "background", backgrounded: false, seq: 2 });
        assert.deepEqual(messages.slice(start).map((message) => message.type), ["background-state", "frame"]);
        assert.equal(messages.at(-2).seq, 2);
        assert.equal(messages.at(-1).pixels.byteLength, messages.at(-1).width * messages.at(-1).height * 4);
        send({ type: "background", backgrounded: false, seq: 3 });
        assert.equal(messages.filter((message) => message.type === "frame").length, 1,
          "resume immediately delivers only the latest frame, without requiring another guest update");
        send({ type: "background", backgrounded: true, seq: 4 });
        send({ type: "host-frame-response", bytes: new Uint8Array([4]) });
        scheduling.drain();
        const staleHandler = receiver.onmessage;
        send({ type: "stop" });
        const stoppedAt = messages.length;
        staleHandler({ data: { type: "background", backgrounded: false, seq: 5 } });
        scheduling.drain();
        assert.equal(messages.length, stoppedAt, "stale handlers and queued ticks cannot replay a stopped frame");
      } finally {
        receiver.onmessage?.({ data: { type: "stop" } });
      }
    });
  }
});

test("initialization continuations cannot consume the automatic first update", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  for (const yielding of [false, true]) {
    for (const idle of [false, true]) {
      await t.test(`yielding=${yielding}, idle=${idle}`, async (t) => {
        const { root } = partitionedGuestBytes({
          meteredHostcalls: { entry: "init", count: 2, cost: 100, idle },
        });
        const compiledProgram = await Runtime.compile(root);
        let calls = 0;
        globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
          constructor(...args) {
            super(...args);
            const exports = this.pvm;
            this.pvm = {
              ...exports,
              pvm_begin: (...args) => {
                calls++;
                if (yielding && calls === 1) this.hostcalls = 1;
                return exports.pvm_begin(...args);
              },
            };
          }
        };
        const scheduling = controlledTicks(t);
        const { messages, receiver } = endpoint();
        try {
          receiver.onmessage({ data: {
            type: "start", runtime: bytesBuffer(runtime), program: new Uint8Array([1]),
            compiledProgram, assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
            cacheKey: `initial-update-${yielding}-${idle}`,
          } });
          assert.equal((await waitForMessage(messages, "ready")).backend, "compiler");
          scheduling.controlTimers();
          assert.equal(calls, 1, "only init has started");
          if (yielding) {
            scheduling.ticks.shift()();
            assert.equal(calls, 1, "the first tick finishes init instead of restarting it");
            assert.equal(scheduling.ticks.length, 1, "first update remains pending after init finishes");
            assert.equal(scheduling.timers.size, 0, "init cannot postpone the first update");
          }
          assert.equal(scheduling.drain(), 1, "the first real update runs exactly once");
          assert.equal(calls, 2);
          assert.equal(scheduling.timers.size, idle ? 0 : 1, "the real update's deadline is honored");
          assert.equal(messages.some((message) => message.type === "error"), false);
        } finally {
          receiver.onmessage?.({ data: { type: "stop" } });
          globalThis.TranslatedPolkaVmRuntime = Runtime;
        }
      });
    }
  }
});

test("foreground wakes survive gas continuations and coalesce at the next call", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const { root } = partitionedGuestBytes({
    meteredHostcalls: { count: 2, cost: 100 },
  });
  const compiledProgram = await Runtime.compile(root);
  for (const event of ["input", "host-response", "resume", "foreground"]) {
    await t.test(event, async (t) => {
      let calls = 0;
      globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
        constructor(...args) {
          super(...args);
          this.maxGas = 100n;
          const exports = this.pvm;
          this.pvm = {
            ...exports,
            pvm_begin: (...args) => {
              calls++;
              return exports.pvm_begin(...args);
            },
          };
        }
      };
      const scheduling = controlledTicks(t);
      const { messages, receiver } = endpoint();
      const send = (data) => receiver.onmessage({ data });
      try {
        send({
          type: "start", runtime: bytesBuffer(runtime), program: new Uint8Array([1]),
          compiledProgram, assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
          cacheKey: `foreground-gas-wake-${event}`,
        });
        assert.equal((await waitForMessage(messages, "ready")).backend, "compiler");
        scheduling.controlTimers();
        scheduling.ticks.shift()();
        assert.equal(calls, 1);
        assert.equal(scheduling.ticks.length, 1, "gas exhaustion schedules a continuation");
        for (let repeat = 0; repeat < 2; repeat++) {
          if (event === "input") {
            send({ type: "input", bytes: new Uint8Array([1, 4, 0, 0, 0, 0, 0, 0]) });
          } else if (event === "host-response") {
            send({ type: "host-frame-response", bytes: new Uint8Array([42]) });
          } else if (event === "resume") {
            send({ type: "pause", paused: true });
            send({ type: "pause", paused: false });
          } else {
            send({ type: "background", backgrounded: true });
            send({ type: "background", backgrounded: false });
          }
        }
        assert.equal(scheduling.ticks.length, 1, "wakes share the pending continuation tick");
        assert.equal(scheduling.drain(), 3, "finish the old call, then run one new two-quantum call");
        assert.equal(calls, 2, "external wakes cannot be spent finishing the old call");
        assert.equal(scheduling.timers.size, 1, "ordinary requested pacing resumes after the wake");
        assert.equal(messages.some((message) => message.type === "error"), false);
      } finally {
        receiver.onmessage?.({ data: { type: "stop" } });
        globalThis.TranslatedPolkaVmRuntime = Runtime;
      }
    });
  }
});

test("foreground host responses arriving after a poll survive hostcall continuations and pause", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
    constructor(...args) {
      super(...args);
      const exports = this.pvm;
      this.pvm = {
        ...exports,
        pvm_begin: (...args) => {
          this.hostcalls = 1;
          return exports.pvm_begin(...args);
        },
        pvm_resume: () => {
          this.hostcalls = 1;
          return exports.pvm_resume();
        },
      };
    }
  };
  const scheduling = controlledTicks(t);
  const { messages, receiver } = endpoint();
  const send = (data) => receiver.onmessage({ data });
  try {
    send({
      type: "start", runtime: bytesBuffer(runtime), program: bytesBuffer(hostResponseEchoGuest(true)),
      assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
      cacheKey: "foreground-hostcall-wake",
    });
    assert.equal((await waitForMessage(messages, "ready")).backend, "compiler");
    scheduling.controlTimers();
    scheduling.ticks.shift()(); // The first call has already polled an empty response queue.
    send({ type: "host-frame-response", bytes: new Uint8Array([42]) });
    send({ type: "pause", paused: true });
    scheduling.drain();
    assert.equal(messages.some((message) => message.type === "save"), false);
    send({ type: "pause", paused: false });
    scheduling.drain();
    assert.deepEqual(messages.filter((message) => message.type === "save").map((message) => message.bytes[0]),
      [0, 42], "the queued response must be observed by a fresh update despite the old call requesting idle");
    assert.equal(scheduling.ticks.length, 0);
    assert.equal(scheduling.timers.size, 0, "consuming a wake cannot create an idle spin");
    assert.equal(messages.some((message) => message.type === "error"), false);
  } finally {
    receiver.onmessage?.({ data: { type: "stop" } });
    globalThis.TranslatedPolkaVmRuntime = Runtime;
  }
});

test("translated background continuations complete bounded hostcall slices without idle spinning", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  for (const clockCalls of [0, 40]) {
    await t.test(clockCalls ? "bounded long continuation" : "complete service continuation", async (t) => {
      // Lower the real hostcall slice budget, not the execution path. The real
      // translated guest resumes through pvm_resume and emits real save data.
      globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
        constructor(...args) {
          super(...args);
          const exports = this.pvm;
          this.pvm = {
            ...exports,
            pvm_begin: (...args) => {
              this.hostcalls = 1;
              return exports.pvm_begin(...args);
            },
            pvm_resume: () => {
              this.hostcalls = 1;
              return exports.pvm_resume();
            },
          };
        }
      };
      const scheduling = controlledTicks(t);
      const { messages, receiver } = endpoint();
      const send = (data) => receiver.onmessage({ data });
      try {
        send({ type: "background", backgrounded: true });
        send({
          type: "start", runtime: bytesBuffer(runtime),
          program: bytesBuffer(hostResponseEchoGuest(true, clockCalls)),
          assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
          cacheKey: `background-continuation-${clockCalls}`,
        });
        assert.equal((await waitForMessage(messages, "ready")).backend, "compiler");
        scheduling.controlTimers();
        send({ type: "host-frame-response", bytes: new Uint8Array([41]) });
        const ticks = scheduling.drain();
        if (clockCalls) {
          assert.equal(ticks, 32, "a pathological continuation must eventually yield to external work");
          assert.equal(messages.some((message) => message.type === "save"), false);
          send({ type: "pause", paused: true });
          send({ type: "host-frame-response", bytes: new Uint8Array([42]) });
          assert.equal(scheduling.ticks.length, 0);
          send({ type: "pause", paused: false });
          scheduling.drain();
        } else {
          assert.equal(ticks, 4, "one credit must finish poll, save and idle hostcall continuations");
        }
        assert.deepEqual(messages.filter((message) => message.type === "save").map((message) => message.bytes[0]), [41]);
        assert.equal(scheduling.timers.size, 0);
        assert.equal(scheduling.ticks.length, 0);
      } finally {
        receiver.onmessage?.({ data: { type: "stop" } });
        globalThis.TranslatedPolkaVmRuntime = Runtime;
      }
    });
  }
});

test("background state rejects malformed booleans and sequences", async () => {
  for (const message of [
    { type: "background", backgrounded: 1 },
    { type: "background", backgrounded: true, seq: -1 },
    { type: "background", backgrounded: true, seq: 1.5 },
    { type: "background", backgrounded: true, seq: Number.MAX_SAFE_INTEGER + 1 },
  ]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({ data: message });
    assert.equal(receiver.onmessage, null);
    // Termination waits for private file-cache cleanup before it is reported.
    await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(messages.map((message) => message.type), ["error", "terminated"]);
  }
});

test("stopping during asynchronous compilation cannot restart the endpoint", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const program = await readFile(resolve(
    repositoryRoot, "rust/crates/polkavm-host-runtime/tests/fixtures/clock-u64.polkavm",
  ));
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const compile = Runtime.compile;
  let release;
  let entered;
  let completed;
  const gate = new Promise((resolve) => { release = resolve; });
  const compiling = new Promise((resolve) => { entered = resolve; });
  const compiled = new Promise((resolve) => { completed = resolve; });
  t.mock.method(Runtime, "compile", async (bytes) => {
    entered();
    await gate;
    const result = await compile.call(Runtime, bytes);
    completed();
    return result;
  });
  const { messages, receiver } = endpoint();
  receiver.onmessage({ data: {
    type: "start", runtime: bytesBuffer(runtime), program: bytesBuffer(program),
    assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
    cacheKey: "stop-during-compile",
  } });
  await compiling;
  receiver.onmessage({ data: { type: "pause", paused: true } });
  receiver.onmessage({ data: { type: "background", backgrounded: true } });
  receiver.onmessage({ data: { type: "stop" } });
  release();
  await compiled;
  await settle();
  assert.equal(receiver.onmessage, null);
  assert.equal(messages.filter((message) => message.type === "terminated").length, 1);
  assert.equal(messages.some((message) => ["ready", "save", "error"].includes(message.type)), false);
});

test("stop remains terminal when pending instantiation resolves or rejects", async (t) => {
  const runtime = await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"));
  const program = await readFile(resolve(
    repositoryRoot, "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
  ));
  for (const reject of [false, true]) {
    await t.test(reject ? "late rejection" : "late resolution", async (t) => {
      const original = WebAssembly.instantiate;
      const entered = Promise.withResolvers();
      const resume = Promise.withResolvers();
      t.mock.method(WebAssembly, "instantiate", async (...args) => {
        const result = await original(...args);
        entered.resolve();
        await resume.promise;
        if (reject) throw new Error("startup failed after cancellation");
        return result;
      });
      const { messages, receiver } = endpoint();
      try {
        receiver.onmessage({ data: {
          type: "start", runtime: bytesBuffer(runtime), program: bytesBuffer(program),
          assets: [], graphicsProfile: "framebuffer", audioEnabled: false,
        } });
        await entered.promise;
        receiver.onmessage({ data: { type: "stop" } });
        await waitForMessage(messages, "terminated");
        const terminalMessages = messages.slice();
        resume.resolve();
        await settle();
        assert.equal(receiver.onmessage, null);
        assert.deepEqual(messages, terminalMessages);
        assert.equal(messages.some((message) => message.type === "ready"), false);
      } finally {
        resume.resolve();
        await settle();
        receiver.onmessage?.({ data: { type: "stop" } });
      }
    });
  }
});
test("both browser backends expose application core clocks and entropy", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/application-core-services.polkavm",
    ),
  );

  for (const forceInterpreter of [false, true]) {
    const wallBefore = BigInt(Date.now()) * 1_000_000n;
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "tri2d",
        audioEnabled: false,
        cacheKey: `application-core-services-${String(forceInterpreter)}`,
        forceInterpreter,
      },
    });

    const save = await waitForMessage(messages, "save");
    const ready = await waitForMessage(messages, "ready");
    assert.equal(ready.backend, forceInterpreter ? "interpreter" : "compiler");
    assert.equal(ready.compilerFallbackReason, undefined);
    assert.equal(ready.compilerFallbackStage, undefined);
    const bytes = new Uint8Array(save.bytes);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const wallAfter = BigInt(Date.now()) * 1_000_000n;
    assert.equal(bytes.byteLength, 56);
    assert.equal(view.getInt32(24, true), 0);
    assert.equal(view.getInt32(28, true), 0);
    assert.equal(view.getInt32(32, true), 0);
    assert.equal(view.getInt32(36, true), 0);
    assert.ok(view.getBigUint64(8, true) >= view.getBigUint64(0, true));
    assert.ok(view.getBigUint64(16, true) >= wallBefore);
    assert.ok(view.getBigUint64(16, true) <= wallAfter);
    assert.ok(bytes.subarray(40).some((byte) => byte !== 0));

    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

for (const bitness of [32, 64]) {
  test(`translated ${bitness}-bit core clock keeps its epoch across init and updates`, async (t) => {
    const runtime = await readFile(
      resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
    );
    const fixture =
      bitness === 32
        ? "application-core-services"
        : "application-core-services-64";
    const program = await readFile(
      resolve(
        repositoryRoot,
        `rust/crates/polkavm-host-runtime/tests/fixtures/${fixture}.polkavm`,
      ),
    );
    const { messages, receiver } = endpoint();
    let compiled;
    try {
      receiver.onmessage({
        data: {
          type: "start",
          runtime: bytesBuffer(runtime),
          program: bytesBuffer(program),
          assets: [],
          graphicsProfile: "framebuffer",
          audioEnabled: false,
          cacheKey: `core-clock-epoch-${bitness}`,
        },
      });
      compiled = await waitForMessage(messages, "compiled");
    } finally {
      // Startup errors already terminate the endpoint and clear its handler.
      if (receiver.onmessage) {
        receiver.onmessage({ data: { type: "stop" } });
        await waitForMessage(messages, "terminated");
      }
    }

    // Model expensive initialization without a timing-sensitive busy wait.
    // Each read also advances within a guest call, independently of frame time.
    let now = 100;
    t.mock.method(performance, "now", () => now++);
    const samples = [];
    const translated = new globalThis.TranslatedPolkaVmRuntime(
      compiled.program,
      [],
      (output) => {
        if (output.type === "save") {
          samples.push(
            new DataView(
              output.bytes.buffer,
              output.bytes.byteOffset,
              output.bytes.byteLength,
            ),
          );
        }
      },
      1_000_000,
      false,
      "framebuffer",
    );
    try {
      now = 141;
      translated.initialize();
      assert.equal(samples.length, 1);
      assert.equal(samples[0].getBigUint64(0, true), 41_000_000n);
      assert.equal(samples[0].getBigUint64(8, true), 42_000_000n);

      // The scheduler starts its epoch after init: 29 ms must not replace 41 ms.
      now = 170;
      translated.update(29);
      assert.equal(samples.length, 2);
      assert.equal(samples[1].getBigUint64(0, true), 70_000_000n);
      assert.equal(samples[1].getBigUint64(8, true), 71_000_000n);

      // An idle interval and even a reset frame clock must not reset core time.
      // This value also exercises both halves of the pointer-based u64 record.
      now = 5_200;
      translated.update(0);
      assert.equal(samples.length, 3);
      assert.equal(samples[2].getBigUint64(0, true), 5_100_000_000n);
      assert.equal(samples[2].getBigUint64(8, true), 5_101_000_000n);
      for (const sample of samples) {
        assert.equal(sample.getInt32(24, true), 0);
        assert.equal(sample.getInt32(28, true), 0);
      }
    } finally {
      translated.stop();
    }
  });
}

test("both browser backends deny unavailable or failing entropy without modifying the destination", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/application-core-services.polkavm",
    ),
  );
  const originalCrypto = Object.getOwnPropertyDescriptor(globalThis, "crypto");
  try {
    for (const browserCrypto of [
      undefined,
      {
        getRandomValues(bytes) {
          bytes.fill(0xa5, 0, Math.ceil(bytes.byteLength / 2));
          throw new Error("entropy provider failed after a partial fill");
        },
      },
    ]) {
      Object.defineProperty(globalThis, "crypto", {
        configurable: true,
        value: browserCrypto,
      });
      for (const forceInterpreter of [false, true]) {
        const { messages, receiver } = endpoint();
        try {
          receiver.onmessage({
            data: {
              type: "start",
              runtime: bytesBuffer(runtime),
              program: bytesBuffer(program),
              assets: [],
              graphicsProfile: "tri2d",
              audioEnabled: false,
              cacheKey: `application-core-services-entropy-failure-${forceInterpreter}`,
              forceInterpreter,
            },
          });
          const ready = await waitForMessage(messages, "ready");
          assert.equal(ready.backend, forceInterpreter ? "interpreter" : "compiler");
          assert.equal(ready.compilerFallbackReason, undefined);
          const save = await waitForMessage(messages, "save");
          const bytes = new Uint8Array(save.bytes);
          const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
          assert.equal(bytes.byteLength, 56);
          assert.equal(view.getInt32(36, true), -5);
          assert.deepEqual(bytes.subarray(40), new Uint8Array(16));
        } finally {
          receiver.onmessage?.({ data: { type: "stop" } });
          await waitForMessage(messages, "terminated");
        }
      }
    }
  } finally {
    if (originalCrypto) {
      Object.defineProperty(globalThis, "crypto", originalCrypto);
    } else {
      delete globalThis.crypto;
    }
  }
});

test("compiler startup keeps the newest GPU capabilities", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  let observedCapabilities;
  globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
    constructor(...args) {
      observedCapabilities = new Uint8Array(args[6]);
      super(...args);
    }
  };
  try {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "webgpu-raster",
        gpuCapabilities: gpuCapabilities(1).buffer,
        audioEnabled: false,
        cacheKey: "gpu-capabilities-startup",
      },
    });
    receiver.onmessage({
      data: {
        type: "gpu-capabilities",
        bytes: gpuCapabilities(2).buffer,
      },
    });
    const ready = await waitForMessage(messages, "ready");
    assert.equal(ready.backend, "compiler");
    assert.equal(
      new DataView(
        observedCapabilities.buffer,
        observedCapabilities.byteOffset,
        observedCapabilities.byteLength,
      ).getUint32(36, true),
      2,
    );
    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  } finally {
    globalThis.TranslatedPolkaVmRuntime = Runtime;
  }
});

test("byte and Module startup round-trip opaque host frames on both backends", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/host-frame-roundtrip.polkavm",
    ),
  );
  const requestBytes = new TextEncoder().encode(
    "host-frame-conformance-request-v1",
  );
  const responseBytes = new TextEncoder().encode(
    "host-frame-conformance-response-v1",
  );
  const successBytes = new TextEncoder().encode("host-frame-roundtrip-ok");

  const runtimeInputs = [bytesBuffer(runtime), await WebAssembly.compile(runtime)];
  for (const { runtimeInput, forceInterpreter } of runtimeInputs.flatMap(
    (runtimeInput) => [false, true].map((forceInterpreter) => ({
      runtimeInput, forceInterpreter,
    })),
  )) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: runtimeInput,
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: `host-frame-roundtrip-${forceInterpreter}`,
        forceInterpreter,
      },
    });

    const request = await waitForMessage(messages, "host-frame-request");
    assert.deepEqual(new Uint8Array(request.bytes), requestBytes);

    receiver.onmessage({
      data: {
        type: "host-frame-response",
        bytes: bytesBuffer(responseBytes),
      },
    });
    const save = await waitForMessage(messages, "save");
    assert.deepEqual(new Uint8Array(save.bytes), successBytes);
    assert.equal(
      messages.some(
        (message) =>
          message.type === "host-frame-response-accepted" ||
          message.type === "host-frame-response-rejected",
      ),
      false,
      "responses without seq must not produce delivery acks",
    );

    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

test("host-frame response backpressure is retryable in both backends", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/host-frame-roundtrip.polkavm",
    ),
  );
  const responseBytes = new TextEncoder().encode(
    "host-frame-conformance-response-v1",
  );

  for (const forceInterpreter of [false, true]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: `host-frame-backpressure-${forceInterpreter}`,
        forceInterpreter,
      },
    });
    await waitForMessage(messages, "host-frame-request");

    const acks = () =>
      messages.filter(
        (message) =>
          message.type === "host-frame-response-accepted" ||
          message.type === "host-frame-response-rejected",
      );

    for (let seq = 0; seq < 33; seq += 1) {
      receiver.onmessage({
        data: {
          type: "host-frame-response",
          bytes: bytesBuffer(responseBytes),
          seq,
        },
      });
    }

    assert.deepEqual(acks(), [
      ...Array.from({ length: 32 }, (_, seq) => ({
        type: "host-frame-response-accepted",
        seq,
      })),
      { type: "host-frame-response-rejected", reason: "queue-full", seq: 32 },
    ]);

    receiver.onmessage({
      data: {
        type: "host-frame-response",
        bytes: bytesBuffer(responseBytes),
      },
    });
    assert.deepEqual(
      messages.findLast(
        (message) => message.type === "host-frame-response-rejected",
      ),
      { type: "host-frame-response-rejected", reason: "queue-full" },
      "responses without seq keep the legacy rejection shape",
    );
    assert.equal(
      messages.some((message) => message.type === "error"),
      false,
    );
    assert.equal(
      messages.some((message) => message.type === "terminated"),
      false,
    );

    await waitForMessage(messages, "save");
    const ackCount = acks().length;
    receiver.onmessage({
      data: {
        type: "host-frame-response",
        bytes: bytesBuffer(responseBytes),
        seq: 32,
      },
    });
    await settle();
    assert.deepEqual(acks().slice(ackCount), [
      { type: "host-frame-response-accepted", seq: 32 },
    ]);
    assert.equal(
      messages.some((message) => message.type === "terminated"),
      false,
    );

    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

test("both browser backends deliver bounded mediated input", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/mediated-input.polkavm",
    ),
  );
  const payload = new TextEncoder().encode("decoded-ur-cbor");

  for (const forceInterpreter of [false, true]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "tri2d",
        audioEnabled: false,
        cacheKey: `mediated-input-${forceInterpreter}`,
        mediatedInputKinds: ["camera-ur"],
        forceInterpreter,
      },
    });

    const request = await waitForMessage(messages, "mediated-input-request");
    assert.equal(request.kind, "camera-ur");
    assert.equal(request.mediaType, "x-test-payload");
    assert.equal(request.maxBytes, 32);
    assert.ok(request.handle > 0);

    const initialSave = await waitForMessage(messages, "save");
    const initialView = new DataView(
      initialSave.bytes.buffer,
      initialSave.bytes.byteOffset,
      initialSave.bytes.byteLength,
    );
    assert.deepEqual(
      [0, 4, 8].map((offset) => initialView.getInt32(offset, true)),
      [request.handle, 0, 2],
    );
    messages.splice(messages.indexOf(initialSave), 1);

    receiver.onmessage({
      data: {
        type: "mediated-input-result",
        handle: request.handle,
        status: 3,
        bytes: bytesBuffer(payload),
      },
    });
    const completedSave = await waitForMessage(messages, "save");
    const completed = new Uint8Array(completedSave.bytes);
    assert.equal(
      new DataView(
        completed.buffer,
        completed.byteOffset,
        completed.byteLength,
      ).getInt32(0, true),
      payload.byteLength,
    );
    assert.deepEqual(completed.subarray(4), payload);

    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

test("both browser backends cancel active mediated input on teardown", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/mediated-input.polkavm",
    ),
  );

  for (const forceInterpreter of [false, true]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "tri2d",
        audioEnabled: false,
        cacheKey: `mediated-input-stop-${forceInterpreter}`,
        mediatedInputKinds: ["camera-ur"],
        forceInterpreter,
      },
    });

    const request = await waitForMessage(messages, "mediated-input-request");
    receiver.onmessage({ data: { type: "stop" } });
    const cancellation = await waitForMessage(
      messages,
      "mediated-input-cancel",
    );
    assert.equal(cancellation.handle, request.handle);
    await waitForMessage(messages, "terminated");
  }
});
test("native-Wasm and translated backends validate and emit UI output v1", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/ui-output.polkavm",
    ),
  );
  const expected = {
    cursorIcon: "text",
    mutableTextUnderCursor: true,
    ime: {
      rect: [10, 20, 210, 60],
      cursorRect: [24, 22, 25, 58],
    },
    commands: [
      { type: "copy-text", text: "hello" },
      {
        type: "open-url",
        url: "https://example.test",
        newSurface: true,
      },
    ],
  };

  for (const forceInterpreter of [false, true]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "tri2d",
        audioEnabled: false,
        cacheKey: `ui-output-${forceInterpreter}`,
        forceInterpreter,
      },
    });

    await waitForMessage(messages, "ready");
    const output = await waitForMessage(messages, "ui-output");
    assert.deepEqual(output.output, expected);

    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

test("compiler backend implements MotionSample v1 status and reads", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "motion-sample-v1",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const outputs = [];
  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    (output) => outputs.push(output),
    1_000_000,
    false,
    "framebuffer",
    null,
    1,
  );
  const sample = motionSample();
  translated.sendMotionSample(sample);
  translated.initialize();
  const written = motionResult(
    outputs.find((output) => output.type === "save").bytes,
  );
  assert.equal(written.status, 48);
  assert.deepEqual(written.sample, sample);
  translated.stop();

  const deniedOutputs = [];
  const denied = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    (output) => deniedOutputs.push(output),
    1_000_000,
    false,
    "framebuffer",
    null,
    2,
  );
  denied.initialize();
  assert.equal(
    motionResult(deniedOutputs.find((output) => output.type === "save").bytes)
      .status,
    -2,
  );
  denied.stop();
});

test("browser endpoint routes motion samples to the interpreter", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "motion-sample-interpreter",
      forceInterpreter: true,
      motionAvailability: 1,
    },
  });
  receiver.onmessage({
    data: { type: "motion", bytes: motionSample().buffer },
  });
  const ready = await waitForMessage(messages, "ready");
  assert.equal(ready.usesMotion, true);
  const result = motionResult((await waitForMessage(messages, "save")).bytes);
  assert.equal(result.status, 48);
  assert.deepEqual(result.sample, motionSample());
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("JIT fallback preserves a motion sample queued during startup", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const warn = console.warn;
  console.warn = () => {};
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
    initialize() {
      throw new Error("forced translated initialization failure");
    }
  };
  try {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: "motion-fallback",
        motionAvailability: 1,
      },
    });
    receiver.onmessage({
      data: { type: "motion", bytes: motionSample().buffer },
    });
    const ready = await waitForMessage(messages, "ready");
    assert.equal(ready.backend, "interpreter");
    assert.equal(
      ready.compilerFallbackReason,
      "forced translated initialization failure",
    );
    assert.equal(ready.compilerFallbackStage, "compiler-initializing");
    assert.equal(ready.usesMotion, true);
    const result = motionResult((await waitForMessage(messages, "save")).bytes);
    assert.equal(result.status, 48);
    assert.deepEqual(result.sample, motionSample());
    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  } finally {
    console.warn = warn;
    globalThis.TranslatedPolkaVmRuntime = Runtime;
  }
});

test("compiler capacity failure selects bounded compiled code and still renders", async (t) => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  const compile = Runtime.compile;
  t.mock.method(Runtime, "compile", async (bytes) => {
    const module = await WebAssembly.compile(bytes);
    if (WebAssembly.Module.customSections(module, "epoca.pvm.code-part").length === 0) {
      throw new RangeError("single-module native code capacity exceeded");
    }
    return compile(bytes);
  });
  t.mock.method(console, "warn", () => {});
  const { messages, receiver } = endpoint();
  try {
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
      },
    });
    assert.equal((await waitForMessage(messages, "ready")).backend, "compiler");
    const frame = await waitForMessage(messages, "frame");
    assert.equal(frame.width, 320);
    assert.equal(frame.height, 200);
    assert.deepEqual(Array.from(frame.pixels.slice(-4)), [0x0f, 0x0f, 0x23, 0xff]);
  } finally {
    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

test("compiler fallback reports root and code-part compilation failures", async (t) => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const { invalidPart } = partitionedGuestBytes();
  for (const [name, compiledBytes] of [
    ["root", new Uint8Array([0])],
    ["code part", invalidPart],
  ]) {
    await t.test(name, async () => {
      let compilationError;
      try {
        await globalThis.TranslatedPolkaVmRuntime.compile(compiledBytes);
      } catch (error) {
        compilationError = error;
      }
      assert.ok(compilationError instanceof WebAssembly.CompileError);
      const { messages, receiver } = endpoint();
      const warn = console.warn;
      console.warn = () => {};
      try {
        receiver.onmessage({
          data: {
            type: "start",
            runtime: bytesBuffer(runtime),
            program: bytesBuffer(program),
            compiledBytes: bytesBuffer(compiledBytes),
            assets: [],
            graphicsProfile: "framebuffer",
            audioEnabled: false,
            cacheKey: `invalid-cached-wasm-${name}`,
          },
        });
        const ready = await waitForMessage(messages, "ready");
        assert.equal(ready.backend, "interpreter");
        assert.equal(ready.compilerFallbackStage, "compiler-compiling");
        assert.equal(ready.compilerFallbackReason, compilationError.message);
        await waitForMessage(messages, "frame");
      } finally {
        console.warn = warn;
        receiver.onmessage({ data: { type: "stop" } });
        await waitForMessage(messages, "terminated");
      }
    });
  }
});

test("cached native code renders when further compilation is unavailable", async (t) => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const start = {
    type: "start",
    runtime: bytesBuffer(runtime),
    program: bytesBuffer(program),
    assets: [],
    graphicsProfile: "framebuffer",
    audioEnabled: false,
    cacheKey: "compiled-program-reuse",
  };
  const first = endpoint();
  let compiledProgram;
  try {
    first.receiver.onmessage({ data: start });
    assert.equal((await waitForMessage(first.messages, "ready")).backend, "compiler");
    compiledProgram = structuredClone(
      (await waitForMessage(first.messages, "compiled")).program,
    );
    await waitForMessage(first.messages, "frame");
  } finally {
    first.receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(first.messages, "terminated");
  }
  t.mock.method(globalThis.TranslatedPolkaVmRuntime, "compile", async () => {
    throw new RangeError("native compiler capacity exhausted");
  });
  const cached = endpoint();
  try {
    cached.receiver.onmessage({ data: { ...start, compiledProgram } });
    const ready = await waitForMessage(cached.messages, "ready");
    assert.equal(ready.backend, "compiler");
    assert.equal(ready.cacheHit, true);
    const frame = await waitForMessage(cached.messages, "frame");
    assert.deepEqual(Array.from(frame.pixels.slice(-4)), [0x0f, 0x0f, 0x23, 0xff]);
  } finally {
    cached.receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(cached.messages, "terminated");
  }
});

test("compiler backend honors CoreVM update deadlines", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/update-schedule-corevm.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "corevm-update-scheduling",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  translated.initialize();
  assert.equal(translated.usesUpdateScheduling(), true);
  translated.update(0);
  assert.equal(translated.updateAfterMilliseconds(), 10);
  translated.update(10);
  assert.equal(translated.updateAfterMilliseconds(), 250);
});

test("interpreter backend starts a CoreVM guest", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/update-schedule-corevm.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "corevm-interpreter",
      forceInterpreter: true,
    },
  });
  const ready = await waitForMessage(messages, "ready");
  assert.equal(ready.backend, "interpreter");
  assert.equal(
    messages.some((message) => message.type === "error"),
    false,
  );
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("compiler backend keeps application time monotonic", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/update-schedule-corevm.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "monotonic-application-time",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  translated.initialize();
  translated.update(50);
  translated.update(10);
  assert.equal(translated.timeMs, 50);
});

test("compiler backend discards stale CoreVM mouse movement", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "mouse-backlog",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  translated.coreVm = true;
  translated.imports = ["pvm_fetch_epoca_inputs"];
  translated.sendInput(pointerDelta(100, -60));
  translated.sendInput(pointerDelta(12, -7));
  translated.sendInput(pointerDelta(430, 314));
  assert.equal(translated.epocaInput.length, 1);
  assert.deepEqual(translated.epocaInput[0], pointerDelta(12, -7));

  translated.imports = [];
  translated.sendInput(pointerDelta(100, 0));
  translated.sendInput(pointerDelta(80, 0));
  assert.deepEqual(translated.coreInput, [[0xa3, 80]]);

  translated.setMotionAvailability(2);
  assert.equal(translated.motionAvailability, 2);
  assert.throws(
    () => translated.sendMotionSample(new Uint8Array(48)),
    /invalid motion sample/,
  );
  const motion = motionSample();
  translated.sendMotionSample(motion);
  assert.equal(translated.motionAvailability, 1);
  assert.deepEqual(translated.motionSample, motion);
});

test("browser runtime can select the interpreter without attempting translation", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "forced-interpreter",
      forceInterpreter: true,
    },
  });

  const ready = await waitForMessage(messages, "ready");
  assert.equal(ready.backend, "interpreter");
  assert.equal(ready.compilerFallbackReason, undefined);
  assert.equal(ready.compilerFallbackStage, undefined);
  assert.equal(ready.usesMotion, false);
  assert.equal(ready.cacheHit, false);
  assert.equal(ready.translationMs, 0);
  assert.equal(ready.compilationMs, 0);
  assert.equal(ready.translatedWasmBytes, 0);
  assert.equal(
    messages.some(
      (message) => message.type === "translated" || message.type === "compiled",
    ),
    false,
  );

  await waitForStartupStage(messages, "first-update-completed");

  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("translated backend keeps pointer capture under Host policy", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "pointer-capture-policy",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  assert.equal(translated.usesPointerCapture(), false);
  assert.equal(translated.takePointerCaptureRequest(), null);

  translated.setPointerCaptureActive(true);
  translated.setPointerCaptureActive(true);
  translated.setPointerCaptureActive(false);
  const records = translated.input.map((record) => [record[0], record[1]]);
  assert.deepEqual(
    records,
    [
      [15, 1],
      [15, 0],
    ],
    "each capture transition reaches the guest exactly once",
  );
  translated.stop();
});

test("both browser backends answer the pointer capture hostcall", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/pointer-capture.polkavm",
    ),
  );
  const status = (bytes) => {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return [0, 4, 8, 12].map((offset) => view.getInt32(offset, true));
  };

  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "pointer-capture-hostcall",
      pointerCaptureSupported: true,
    },
  });
  const ready = await waitForMessage(messages, "ready");
  assert.equal(
    ready.usesPointerCapture,
    true,
    "the Host learns that this guest arms capture itself",
  );
  const saved = await waitForMessage(messages, "save");
  assert.deepEqual(
    status(saved.bytes),
    [1, -2, 0, 1],
    "arm, undefined request, release, arm",
  );
  const request = await waitForMessage(messages, "pointer-capture");
  assert.equal(
    request.capture,
    true,
    "the newest guest request reaches the Host",
  );
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const outputs = [];
  const unsupported = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    (output) => outputs.push(output),
    1_000_000,
    false,
    "framebuffer",
  );
  assert.equal(unsupported.usesPointerCapture(), true);
  unsupported.initialize();
  assert.deepEqual(
    status(outputs.find((output) => output.type === "save").bytes),
    [-1, -1, -1, -1],
    "a backend without capture support answers every request alike",
  );
  assert.equal(unsupported.takePointerCaptureRequest(), null);
  unsupported.stop();

  const supportedOutputs = [];
  const supported = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    (output) => supportedOutputs.push(output),
    1_000_000,
    false,
    "framebuffer",
  );
  supported.setPointerCaptureSupported(true);
  supported.initialize();
  assert.deepEqual(
    status(supportedOutputs.find((output) => output.type === "save").bytes),
    [1, -2, 0, 1],
    "the translated backend matches the native status codes",
  );
  assert.equal(supported.takePointerCaptureRequest(), true);
  supported.setPointerCaptureSupported(false);
  supported.initialize();
  assert.equal(
    supported.takePointerCaptureRequest(),
    null,
    "revoking support drops the request the Host has not served",
  );
  supported.stop();
});

test("both browser backends take viewport insets as a whole pair", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "view-insets",
    },
  });
  await waitForMessage(messages, "ready");
  receiver.onmessage({
    data: {
      type: "view-insets",
      eventType: 16,
      left: 0,
      top: 47,
      right: 0,
      bottom: 34,
    },
  });
  receiver.onmessage({
    data: {
      type: "view-insets",
      eventType: 17,
      left: 0,
      top: 0,
      right: 0,
      bottom: 640,
    },
  });
  await settle();
  assert.equal(
    messages.some((message) => message.type === "error"),
    false,
    "the interpreter accepts safe-area and keyboard updates",
  );

  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  translated.sendViewInsets(16, 0, 47, 0, 34);
  translated.sendViewInsets(16, 0, 132, 0, 34);
  translated.sendViewInsets(17, 0, 0, 0, 640);
  const queued = translated.input.map((record) => [
    record[0],
    record[1],
    record[2] | (record[3] << 8),
    record[4] | (record[5] << 8),
  ]);
  assert.deepEqual(
    queued,
    [
      [16, 0, 0, 0],
      [16, 1, 132, 34],
      [17, 0, 0, 0],
      [17, 1, 0, 640],
    ],
    "the newest update of each source supersedes the queued one, axes intact",
  );
  translated.stop();
});

test("a guest buffer that ends mid-pair waits for the whole update", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "view-insets-poll",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.program,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  translated.sendInput(new Uint8Array([1, 4, 0, 0, 0, 0, 0, 0]));
  translated.sendViewInsets(16, 0, 47, 0, 34);
  assert.equal(
    translated.pollableInputCount(2),
    1,
    "a buffer that ends between the axes takes the key only",
  );
  assert.equal(
    translated.pollableInputCount(3),
    3,
    "room for both axes delivers the whole update",
  );
  assert.equal(
    translated.pollableInputCount(1),
    1,
    "a buffer too small for a pair still makes progress",
  );
  translated.stop();
});

test("the browser endpoint rejects malformed viewport insets", async () => {
  for (const message of [
    {
      type: "view-insets",
      eventType: 15,
      left: 0,
      top: 0,
      right: 0,
      bottom: 0,
    },
    {
      type: "view-insets",
      eventType: 16,
      left: -1,
      top: 0,
      right: 0,
      bottom: 0,
    },
    {
      type: "view-insets",
      eventType: 16,
      left: 0,
      top: 0.5,
      right: 0,
      bottom: 0,
    },
    {
      type: "view-insets",
      eventType: 17,
      left: 0,
      top: 0,
      right: 0,
      bottom: 65_536,
    },
  ]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({ data: message });
    await settle();
    assert.equal(
      messages.some((posted) => posted.type === "error"),
      true,
      `an out-of-contract inset update is refused: ${JSON.stringify(message)}`,
    );
  }
});

test("an inset record cannot enter through the ordinary input channel", async () => {
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "input",
      bytes: bytesBuffer(new Uint8Array([16, 0, 10, 0, 20, 0, 0, 0])),
    },
  });
  await settle();
  const error = messages.find((message) => message.type === "error");
  assert.match(
    error?.message ?? "",
    /view-insets message/,
    "a lone axis is refused instead of tearing the pair",
  );
});
