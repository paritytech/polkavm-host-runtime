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

function partitionedGuestBytes() {
  const uleb = (value) => {
    const bytes = [];
    do {
      const byte = value & 0x7f;
      value >>>= 7;
      bytes.push(byte | (value ? 0x80 : 0));
    } while (value);
    return bytes;
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
  const metadata = [
    ...new TextEncoder().encode("EPM2"),
    1, 0, 0, 0, // 64-bit registers
    ...new Array(11 * 4).fill(0), // no hostcall address translation needed
    0, 0, 0, 0, // imports
    2, 0, 0, 0, // exports
    ...metadataExport("init"),
    ...metadataExport("update"),
  ];
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
    section(3, [3, 0, 1, 2]),
    section(4, [1, 0x70, 0, 2]),
    section(5, [1, 0, 1]),
    section(
      6,
      vector(Array.from({ length: 14 }, () => [0x7e, 1, 0x42, 0, 0x0b])),
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
        ...Array.from({ length: 13 }, (_, index) =>
          exportEntry(`r${index}`, 3, index),
        ),
      ]),
    ),
    section(
      10,
      vector([
        body([0x41, 0, 0x28, 2, 0]), // helper reads memory[0]
        body([0x20, 1, 0x24, 13, 0x20, 0, 0x13, 0, 0]),
        body([0x20, 0, 0x24, 13]),
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

test("graphics runtimes expose wall clock and secure random core services", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/application-core-services.polkavm",
    ),
  );
  const success = new TextEncoder().encode("application-core-services-ok");

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
        cacheKey: `application-core-services-${String(forceInterpreter)}`,
        forceInterpreter,
      },
    });

    const save = await waitForMessage(messages, "save");
    assert.deepEqual(save.bytes, success);
    const ready = await waitForMessage(messages, "ready");
    assert.equal(ready.backend, forceInterpreter ? "interpreter" : "compiler");
    assert.equal(ready.compilerFallbackReason, undefined);
    assert.equal(ready.compilerFallbackStage, undefined);
    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
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

test("native-Wasm and translated backends round-trip opaque host frames", async () => {
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
