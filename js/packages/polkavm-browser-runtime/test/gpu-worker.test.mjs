import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(
  resolve(import.meta.dirname, "../src/polkavm-gpu-worker.js"),
  "utf8",
);
const context = vm.createContext({
  ArrayBuffer,
  DataView,
  Map,
  Set,
  TextDecoder,
  TextEncoder,
  Uint8Array,
  onmessage: null,
  postMessage() {},
  performance: { now: () => 0 },
  setTimeout(callback) {
    queueMicrotask(callback);
  },
  GPUTextureUsage: { RENDER_ATTACHMENT: 0x10, COPY_SRC: 0x01, COPY_DST: 0x02 },
  GPUBufferUsage: { COPY_DST: 0x08, MAP_READ: 0x01, COPY_SRC: 0x04, QUERY_RESOLVE: 0x200 },
  GPUMapMode: { READ: 0x01 },
});
vm.runInContext(
  `${source}\nglobalThis.gpuWorkerTest = { GpuEngine, parseCommand, parseCommands };`,
  context,
);
const { GpuEngine, parseCommand, parseCommands } = context.gpuWorkerTest;

function parse(opcode, payload) {
  return parseCommand({ opcode, payload, index: 0 });
}

function shaderCommands(count) {
  const wgsl = new TextEncoder().encode("@vertex fn main() {}");
  return Array.from({ length: count }, (_, index) => {
    const payload = new Uint8Array(8 + wgsl.byteLength);
    const view = new DataView(payload.buffer);
    view.setUint32(0, (1 << 20) | (index + 1), true);
    view.setUint32(4, wgsl.byteLength, true);
    payload.set(wgsl, 8);
    return { opcode: 6, payload, index };
  });
}

test("parses the R8Unorm texture format", () => {
  const payload = new Uint8Array(24);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 64, true);
  view.setUint32(8, 64, true);
  view.setUint16(12, 1, true);
  view.setUint16(14, 1, true);
  view.setUint16(16, 7, true);
  view.setUint8(18, 1);
  view.setUint32(20, 4, true);

  assert.equal(parse(3, payload).format, "r8unorm");
});

test("parses read-only storage buffer layouts", () => {
  const payload = new Uint8Array(40);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, 3, true);
  view.setUint32(12, 3, true);
  view.setUint16(16, 4, true);
  view.setBigUint64(24, 16n, true);

  const [entry] = parse(7, payload).entries;
  assert.equal(entry.binding, 3);
  assert.equal(entry.buffer.type, "read-only-storage");
  assert.equal(entry.buffer.minBindingSize, 16);

  view.setUint16(18, 2, true);
  assert.throws(() => parse(7, payload), /invalid buffer binding layout/);
});

test("parses writable storage buffer layouts", () => {
  const payload = new Uint8Array(40);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, 3, true);
  view.setUint32(12, 4, true);
  view.setUint16(16, 5, true);
  view.setBigUint64(24, 16n, true);

  const [entry] = parse(7, payload).entries;
  assert.equal(entry.binding, 3);
  assert.equal(entry.buffer.type, "storage");
  assert.equal(entry.buffer.minBindingSize, 16);
});

test("rejects writable storage buffer layouts in the vertex stage", () => {
  const payload = new Uint8Array(40);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, 3, true);
  view.setUint32(12, 1, true);
  view.setUint16(16, 5, true);
  view.setBigUint64(24, 16n, true);

  assert.throws(() => parse(7, payload), /invalid buffer binding layout/);
});

test("validates compute pipeline dispatch batches", () => {
  const shader = new TextEncoder().encode(
    "@compute @workgroup_size(1) fn cs_main() {}",
  );
  const shaderPayload = new Uint8Array(
    8 + Math.ceil(shader.byteLength / 4) * 4,
  );
  const shaderView = new DataView(shaderPayload.buffer);
  shaderView.setUint32(0, handle(1), true);
  shaderView.setUint32(4, shader.byteLength, true);
  shaderPayload.set(shader, 8);
  const layoutPayload = u32s([handle(3), 0]);
  const computePipelinePayload = u32s([handle(4), handle(3), handle(1), 0]);
  const batch = commands([
    [6, shaderPayload],
    [8, layoutPayload],
    [24, computePipelinePayload],
    [25, new Uint8Array()],
    [26, u32s([handle(4)])],
    [28, u32s([1, 1, 1])],
    [29, new Uint8Array()],
  ]);
  const engine = validationEngine();

  const validated = engine.validate(parseCommands(batch));

  assert.equal(validated.commands.at(-2).opcode, 28);
});

test("rejects nested render pass inside compute pass", () => {
  const batch = commands([
    [25, new Uint8Array()],
    [12, u32s([0, 0, 1, 0, 0, 0, 0, 0x3f800000, 0x3f800000])],
  ]);
  const engine = validationEngine();

  assert.throws(() => engine.validate(parseCommands(batch)), /nested GPU pass/);
});

test("completes a validated batch without waiting for the GPU queue", async () => {
  let fences = 0;
  const engine = Object.create(GpuEngine.prototype);
  Object.assign(engine, {
    stopped: false,
    resources: new Map(),
    handleSlots: new Map(),
    lastSequence: 0n,
    testReadbacksRemaining: 0,
    testDeviceLossPending: false,
    device: {
      pushErrorScope() {},
      popErrorScope: async () => null,
      queue: {
        onSubmittedWorkDone: async () => {
          fences++;
        },
      },
    },
  });
  engine.validate = () => ({
    commands: [],
    slots: new Map(),
  });
  const batch = new Uint8Array(24);
  const view = new DataView(batch.buffer);
  batch.set(new TextEncoder().encode("EPG1"));
  view.setUint16(4, 1, true);
  view.setUint32(8, batch.byteLength, true);
  view.setBigUint64(16, 1n, true);

  await engine.execute(batch);

  assert.equal(fences, 0);
  assert.equal(engine.lastSequence, 1);
});

function handle(slot) {
  return (1 << 20) | slot;
}

function u32s(values) {
  const bytes = new Uint8Array(values.length * 4);
  const view = new DataView(bytes.buffer);
  values.forEach((value, index) => view.setUint32(index * 4, value, true));
  return bytes;
}

function validationEngine() {
  return Object.assign(Object.create(GpuEngine.prototype), {
    resources: new Map(),
    handleSlots: new Map(),
    lastSequence: 0n,
    occlusionReadbacks: new Set(),
    occlusionEpoch: 0,
    occlusionDelivery: Promise.resolve(),
    queue: Promise.resolve(),
    pendingBatches: 0,
    stopped: false,
    disposed: false,
    limits: [
      4096,
      16 * 1024 * 1024,
      16,
      4,
      8,
      16,
      4,
      256 * 1024 * 1024,
      64 * 1024 * 1024,
      8192,
      4 * 1024 * 1024,
      16 * 1024 * 1024,
      16 * 1024 * 1024,
      8,
      16 * 1024,
      256,
      256,
      256,
      64,
      65_535,
      8192,
      1,
      256,
      256,
    ],
    surfaceGeneration: 1,
  });
}

function commands(items, sequence = 1n) {
  const commandBytes = items.reduce(
    (total, [, payload]) => total + 8 + payload.byteLength,
    0,
  );
  const bytes = new Uint8Array(24 + commandBytes);
  const view = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode("EPG1"));
  view.setUint16(4, 1, true);
  view.setUint32(8, bytes.byteLength, true);
  view.setUint32(12, items.length, true);
  view.setBigUint64(16, sequence, true);
  let offset = 24;
  for (const [opcode, payload] of items) {
    view.setUint16(offset, opcode, true);
    view.setUint32(offset + 4, 8 + payload.byteLength, true);
    bytes.set(payload, offset + 8);
    offset += 8 + payload.byteLength;
  }
  return bytes;
}

test("validates the GPUI editor's nineteen-compilation init batch", () => {
  const engine = validationEngine();
  engine.validate({ sequence: 1, commands: shaderCommands(19) });
});

test("rejects more compilations than the per-batch bound", () => {
  const engine = validationEngine();
  assert.throws(
    () => engine.validate({ sequence: 1, commands: shaderCommands(33) }),
    /too many GPU compilations/,
  );
});

function offscreenTextureCommands() {
  const texture = new Uint8Array(24);
  const textureView = new DataView(texture.buffer);
  textureView.setUint32(0, handle(1), true);
  textureView.setUint32(4, 64, true);
  textureView.setUint32(8, 64, true);
  textureView.setUint16(12, 1, true);
  textureView.setUint16(14, 1, true);
  textureView.setUint16(16, 7, true); // R8Unorm path coverage texture.
  textureView.setUint8(18, 1);
  textureView.setUint32(20, 0x14, true); // TEXTURE_BINDING | RENDER_ATTACHMENT.

  const view = new Uint8Array(20);
  const descriptor = new DataView(view.buffer);
  descriptor.setUint32(0, handle(2), true);
  descriptor.setUint32(4, handle(1), true);
  descriptor.setUint16(8, 7, true);
  descriptor.setUint8(10, 1);
  descriptor.setUint8(11, 1);
  descriptor.setUint16(14, 1, true);
  descriptor.setUint16(18, 1, true);
  return [[3, texture], [23, view]];
}

function renderPass(colorView, flags = 2, generation = 1) {
  return [
    [12, u32s([colorView, 0, generation, flags, 0, 0, 0, 0x3f800000, 0x3f800000])],
    [21, new Uint8Array()],
  ];
}

test("renders to a new texture without acquiring the surface and reuses it in a later batch", async () => {
  const engine = validationEngine();
  const offscreenView = {};
  const surfaceView = {};
  const attachments = [];
  let surfaceAcquisitions = 0;
  let submissions = 0;
  Object.assign(engine, {
    context: {
      getCurrentTexture() {
        surfaceAcquisitions++;
        return { createView: () => surfaceView };
      },
    },
    device: {
      pushErrorScope() {},
      popErrorScope: async () => null,
      createTexture: () => ({ createView: () => offscreenView }),
      createCommandEncoder: () => ({
        beginRenderPass(descriptor) {
          attachments.push(descriptor.colorAttachments[0]);
          return { end() {} };
        },
        finish: () => ({}),
      }),
      queue: {
        submit() { submissions++; },
        onSubmittedWorkDone: async () => {},
      },
    },
    emitBatchRejected() {
      assert.fail("valid attachment batch was rejected");
    },
  });

  await engine.execute(commands([
    ...offscreenTextureCommands(),
    ...renderPass(handle(2)),
  ]));

  assert.equal(surfaceAcquisitions, 0);
  assert.equal(submissions, 1);
  assert.equal(attachments[0].view, offscreenView);
  assert.equal(attachments[0].loadOp, "clear");
  assert.equal(attachments[0].storeOp, "store");

  await engine.execute(commands([
    ...renderPass(handle(2), 3),
    ...renderPass(0),
    ...renderPass(0, 3),
  ], 2n));

  assert.equal(submissions, 2);
  assert.equal(engine.lastSequence, 2);
  assert.equal(attachments[1].view, offscreenView);
  assert.equal(attachments[1].loadOp, "load");
  assert.equal(surfaceAcquisitions, 1);
  assert.equal(attachments[2].view, surfaceView);
  assert.equal(attachments[2].loadOp, "clear");
  assert.equal(attachments[3].view, surfaceView);
  assert.equal(attachments[3].loadOp, "load");
});

function backgroundEngine() {
  const textures = [];
  const visibleFrames = [];
  const canvas = {};
  const makeTexture = descriptor => {
    const texture = {
      descriptor,
      destroyed: false,
      pixel: [0, 0, 0, 0],
      createView() {
        assert.equal(this.destroyed, false, "attachments must remain live");
        return { texture: this };
      },
      destroy() { this.destroyed = true; },
    };
    return texture;
  };
  const device = {
    addEventListener() {},
    lost: new Promise(() => {}),
    destroy() {},
    pushErrorScope() {},
    popErrorScope: async () => null,
    createTexture(descriptor) {
      const texture = makeTexture(descriptor);
      textures.push(texture);
      return texture;
    },
    createBuffer({ size }) {
      const bytes = new Uint8Array(size);
      return {
        bytes,
        mapAsync: async () => {},
        getMappedRange: () => bytes.buffer,
        unmap() {},
        destroy() {},
      };
    },
    createCommandEncoder() {
      const operations = [];
      return {
        beginRenderPass({ colorAttachments: [attachment] }) {
          operations.push(() => {
            const texture = attachment.view.texture;
            assert.equal(texture.destroyed, false);
            if (attachment.loadOp === "clear") {
              const { r, g, b, a } = attachment.clearValue;
              texture.pixel = [r, g, b, a].map(value => Math.round(value * 255));
            }
          });
          return { end() {} };
        },
        copyTextureToBuffer({ texture }, { buffer, offset }) {
          operations.push(() => buffer.bytes.set(texture.pixel, offset));
        },
        copyTextureToTexture({ texture: source }, { texture: destination }) {
          operations.push(() => {
            assert.equal(source.destroyed, false);
            destination.pixel = [...source.pixel];
          });
        },
        finish: () => operations,
      };
    },
    queue: {
      submit(batches) {
        for (const operations of batches) {
          for (const operation of operations) operation();
        }
      },
    },
  };
  const engine = new GpuEngine(
    canvas,
    device,
    {
      configure() {},
      getCurrentTexture() {
        const texture = makeTexture({ size: [canvas.width, canvas.height, 1] });
        visibleFrames.push(texture);
        return texture;
      },
    },
    "rgba8unorm",
    validationEngine().limits,
    { physicalWidth: 64, physicalHeight: 64, logicalWidth: 64, logicalHeight: 64, scale: 1 },
    true,
    false,
    {},
  );
  return { engine, textures, visibleFrames };
}

test("background batches retain resources, readbacks and ordered completion without presenting", async () => {
  const { engine, textures, visibleFrames } = backgroundEngine();
  const capture = captureMessages();
  try {
    engine.setBackground(true);
    engine.submit(commands([
      ...offscreenTextureCommands(),
      ...renderPass(handle(2)),
      ...renderPass(0),
    ]));
    const greenSurface = renderPass(0);
    new DataView(greenSurface[0][1].buffer).setFloat32(20, 1, true);
    engine.submit(commands([
      ...renderPass(handle(2), 3),
      ...greenSurface,
    ], 2n));
    await engine.queue;

    assert.equal(visibleFrames.length, 0, "hidden work must not acquire a swapchain texture");
    assert.equal(textures.length, 2, "one guest texture and one reused background surface");
    assert.deepEqual(
      capture.messages.filter(message => message.type === "event").map(message => [
        eventType(message.bytes),
        new DataView(message.bytes.buffer).getBigUint64(16, true),
      ]),
      [[5, 1n], [5, 2n]],
      "both batches finish in order without rejection",
    );
    const readbacks = capture.messages.filter(message => message.type === "test-readback");
    assert.equal(readbacks.length, 2);
    assert.deepEqual(
      readbacks.map(message => Array.from(message.samples, sample => Array.from(sample))),
      [
        [[0, 0, 0, 255], [0, 0, 0, 255], [0, 0, 0, 255]],
        [[0, 255, 0, 255], [0, 255, 0, 255], [0, 255, 0, 255]],
      ],
    );
    assert.equal(capture.messages.some(message => message.type === "presented"), false);

    engine.setBackground(false);
    await engine.queue;
    assert.equal(visibleFrames.length, 1, "idle guests resume without a new batch");
    assert.deepEqual(visibleFrames[0].pixel, [0, 255, 0, 255], "resume copies the latest hidden frame");
    assert.equal(capture.messages.filter(message => message.type === "event").length, 2);
    assert.deepEqual(
      capture.messages.filter(message => message.type === "presented").map(message => message.sequence),
      [2],
      "the resume copy signals a real presentation without another guest completion",
    );
    await engine.execute(commands([
      ...renderPass(handle(2), 3),
      ...renderPass(0),
    ], 3n));
    assert.equal(visibleFrames.length, 2);
    assert.deepEqual(visibleFrames[1].pixel, [0, 0, 0, 255]);
    assert.deepEqual(
      capture.messages.filter(message => message.type === "presented").map(message => message.sequence),
      [2, 3],
    );
  } finally {
    capture.restore();
    engine.stop();
  }
});

test("non-surface background work preserves a snapshot but discarded surface contents never resume", async () => {
  const { engine, visibleFrames } = backgroundEngine();
  const capture = captureMessages();
  try {
    engine.setBackground(true);
    const greenSurface = renderPass(0);
    new DataView(greenSurface[0][1].buffer).setFloat32(20, 1, true);
    engine.submit(commands(greenSurface));
    engine.submit(commands(offscreenTextureCommands(), 2n));
    engine.setBackground(false);
    await engine.queue;
    assert.deepEqual(visibleFrames.map(frame => frame.pixel), [[0, 255, 0, 255]]);
    engine.setBackground(true);
    engine.submit(commands(renderPass(0, 0), 3n));
    engine.setBackground(false);
    await engine.queue;
    assert.deepEqual(visibleFrames.map(frame => frame.pixel), [[0, 255, 0, 255]],
      "discarded surface contents must not replace the last visible frame");
    assert.deepEqual(
      capture.messages.filter(message => message.type === "event").map(message => [
        eventType(message.bytes),
        new DataView(message.bytes.buffer).getBigUint64(16, true),
      ]),
      [[5, 1n], [5, 2n], [5, 3n]],
    );
  } finally {
    capture.restore();
    engine.stop();
  }
});

test("background targets follow resize and are released on reset and stop", async () => {
  const { engine, textures, visibleFrames } = backgroundEngine();
  engine.setBackground(true);
  await engine.execute(commands(renderPass(0)));
  engine.scheduleResize({
    physicalWidth: 128, physicalHeight: 96, logicalWidth: 128, logicalHeight: 96, scale: 1,
  });
  engine.submit(commands(renderPass(0, 2, 2), 2n));
  await engine.queue;
  assert.equal(textures[0].destroyed, true);
  assert.deepEqual(Array.from(textures[1].descriptor.size), [128, 96, 1]);
  assert.equal(textures.filter(texture => !texture.destroyed).length, 1);
  assert.equal(visibleFrames.length, 0);

  engine.setBackground(false);
  await engine.queue;
  await engine.execute(commands(renderPass(0, 2, 2), 3n));
  assert.deepEqual(visibleFrames[0].descriptor.size, [128, 96, 1]);
  engine.reset();
  await engine.queue;
  assert.equal(textures.every(texture => texture.destroyed), true);
  engine.setBackground(true);
  await engine.execute(commands(renderPass(0, 2, 2)));
  engine.stop();
  assert.equal(textures.every(texture => texture.destroyed), true);
});

test("hiding during batch completion suppresses presentation metrics without dropping completion", async () => {
  const { engine } = backgroundEngine();
  let releaseValidation;
  const validation = new Promise(resolve => { releaseValidation = resolve; });
  engine.device.popErrorScope = () => validation;
  const capture = captureMessages();
  try {
    const executing = engine.execute(commands(renderPass(0)));
    engine.setBackground(true);
    releaseValidation(null);
    await executing;
    assert.equal(capture.messages.some(message => message.type === "presented"), false);
    assert.deepEqual(
      capture.messages.filter(message => message.type === "event").map(message => eventType(message.bytes)),
      [5],
    );
  } finally {
    capture.restore();
    engine.stop();
  }
});

test("rapid background transitions do not publish while hidden or replay stale snapshots", async () => {
  const { engine, visibleFrames } = backgroundEngine();
  engine.setBackground(true);
  engine.submit(commands(renderPass(0)));
  engine.setBackground(false);
  engine.setBackground(true);
  await engine.queue;
  assert.equal(visibleFrames.length, 0);
  engine.setBackground(false);
  await engine.queue;
  assert.equal(visibleFrames.length, 1);
  engine.setBackground(true);
  engine.setBackground(false);
  await engine.queue;
  assert.equal(visibleFrames.length, 1, "an empty hidden interval has no snapshot to replay");
  engine.stop();
});

test("restoring a device invalidates the old background snapshot and keeps new work hidden", async () => {
  const old = backgroundEngine();
  const replacement = backgroundEngine();
  const engine = old.engine;
  engine.setBackground(true);
  await engine.execute(commands(renderPass(0)));
  const acquire = GpuEngine.acquireDevice;
  GpuEngine.acquireDevice = async () => ({
    device: replacement.engine.device,
    context: replacement.engine.context,
    format: replacement.engine.format,
    limits: replacement.engine.limits,
  });
  const capture = captureMessages();
  try {
    engine.stopped = true;
    await engine.restore();
    assert.equal(old.textures[0].destroyed, true);
    await engine.execute(commands(renderPass(0)));
    assert.equal(replacement.visibleFrames.length, 0);
    engine.setBackground(false);
    await engine.queue;
    assert.deepEqual(replacement.visibleFrames[0].pixel, [0, 0, 0, 255]);
    assert.deepEqual(
      capture.messages.filter(message => message.type === "event").map(message => eventType(message.bytes)),
      [8, 5],
      "restoration and batch completion remain guest-visible while hidden",
    );
  } finally {
    GpuEngine.acquireDevice = acquire;
    capture.restore();
    engine.stop();
    replacement.engine.stop();
  }
});

test("rejects missing, wrong-type and deleted color attachments before any GPU mutation", async t => {
  for (const [name, colorView, removeView] of [
    ["missing", handle(3), false],
    ["wrong type", handle(1), false],
    ["deleted", handle(2), true],
  ]) {
    await t.test(name, async () => {
      const engine = validationEngine();
      const rejected = [];
      engine.device = {
        pushErrorScope() { assert.fail("invalid batch reached the GPU"); },
      };
      engine.emitBatchRejected = (...args) => rejected.push(args);
      const setup = offscreenTextureCommands();
      if (removeView) {
        setup.push([11, u32s([handle(2)])]);
      }

      await engine.execute(commands([...setup, ...renderPass(colorView)]));

      assert.equal(rejected.length, 1);
      assert.equal(rejected[0][0], setup.length);
      assert.equal(rejected[0][1], 1);
      assert.equal(rejected[0][2], 1);
      assert.equal(engine.resources.size, 0);
      assert.equal(engine.handleSlots.size, 0);
      assert.equal(engine.lastSequence, 0n);
    });
  }
});

test("requires the current surface generation for both texture and surface attachments", () => {
  for (const colorView of [0, handle(2)]) {
    const engine = validationEngine();
    assert.throws(
      () => engine.validate(parseCommands(commands([
        ...offscreenTextureCommands(),
        ...renderPass(colorView, 2, 2),
      ]))),
      error => error.commandIndex === 2 && error.errorCode === 4,
    );
  }
});

function eventType(bytes) {
  return new DataView(
    bytes.buffer,
    bytes.byteOffset,
    bytes.byteLength,
  ).getUint16(6, true);
}

function lostEngine(overrides = {}) {
  const engine = Object.create(GpuEngine.prototype);
  Object.assign(engine, {
    canvas: { width: 320, height: 240 },
    requirements: {},
    // A lost device leaves the engine parked with its dead resource table.
    resources: new Map([[1, { value: { destroy() {} } }]]),
    handleSlots: new Map([[1, 1]]),
    limits: Array.from({ length: 21 }, () => 4096),
    physicalWidth: 320,
    physicalHeight: 240,
    logicalWidth: 320,
    logicalHeight: 240,
    scale: 1,
    formatId: 1,
    format: "bgra8unorm",
    surfaceGeneration: 1,
    deviceGeneration: 1,
    lastSequence: 7,
    pendingBatches: 2,
    testReadbacksRemaining: 0,
    testDeviceLossPending: false,
    stopped: true,
    disposed: false,
    restoreAttempts: 0,
    restoreInProgress: false,
    restoreFailed: false,
    deviceRestoredAt: null,
    backgroundRequested: false,
    backgrounded: false,
    occlusionEpoch: 0,
    occlusionReadbacks: new Set(),
    occlusionDelivery: Promise.resolve(),
    device: { destroy() {} },
    context: { configure() {} },
    ...overrides,
  });
  return engine;
}

function captureMessages() {
  const messages = [];
  const previous = context.postMessage;
  context.postMessage = (message) => messages.push(message);
  return {
    messages,
    restore() {
      context.postMessage = previous;
    },
  };
}

test("a rebuilt device is published to the guest with fresh capabilities", async () => {
  const engine = lostEngine();
  const replacement = {
    device: {
      addEventListener() {},
      lost: new Promise(() => {}),
      destroy() {},
    },
    context: { configure() {} },
    format: "bgra8unorm",
    limits: Array.from({ length: 21 }, () => 2048),
  };
  const acquire = GpuEngine.acquireDevice;
  GpuEngine.acquireDevice = async () => replacement;
  const capture = captureMessages();
  try {
    await engine.restore();
  } finally {
    GpuEngine.acquireDevice = acquire;
    capture.restore();
  }

  assert.equal(engine.stopped, false, "the engine accepts batches again");
  assert.equal(engine.device, replacement.device);
  assert.equal(engine.deviceGeneration, 2, "the guest can see a new device");
  assert.equal(engine.resources.size, 0, "handles died with the old device");
  assert.equal(engine.handleSlots.size, 0);
  assert.equal(engine.lastSequence, 0, "submission sequencing restarts");
  assert.equal(engine.pendingBatches, 0);
  assert.deepEqual(
    capture.messages.map((message) => message.type),
    ["capabilities", "event"],
    "capabilities land before the restored event so a guest can read them",
  );
  assert.equal(eventType(capture.messages[1].bytes), 8);
});

test("a temporarily unavailable adapter is retried before reporting failure", async () => {
  const engine = lostEngine();
  const replacement = {
    device: {
      addEventListener() {},
      lost: new Promise(() => {}),
      destroy() {},
    },
    context: { configure() {} },
    format: "bgra8unorm",
    limits: Array.from({ length: 21 }, () => 2048),
  };
  let attempts = 0;
  const acquire = GpuEngine.acquireDevice;
  GpuEngine.acquireDevice = async () => {
    attempts++;
    if (attempts < 3) {
      throw new Error("WebGPU adapter is unavailable");
    }
    return replacement;
  };
  const capture = captureMessages();
  try {
    await engine.restore();
  } finally {
    GpuEngine.acquireDevice = acquire;
    capture.restore();
  }

  assert.equal(attempts, 3);
  assert.equal(engine.stopped, false, "the recovered surface accepts batches");
  assert.deepEqual(
    capture.messages.map(message => message.type),
    ["capabilities", "event"]
  );
});

test("a permanently unavailable adapter reports one error after bounded retries", async () => {
  const engine = lostEngine();
  let attempts = 0;
  const acquire = GpuEngine.acquireDevice;
  GpuEngine.acquireDevice = async () => {
    attempts++;
    throw new Error("WebGPU adapter is unavailable");
  };
  const capture = captureMessages();
  try {
    await engine.restore();
  } finally {
    GpuEngine.acquireDevice = acquire;
    capture.restore();
  }

  assert.equal(attempts, 3, "the retry ceiling is honoured");
  assert.equal(engine.stopped, true, "the surface stays down");
  assert.equal(engine.deviceGeneration, 1);
  assert.deepEqual(
    capture.messages.map(message => message.type),
    ["error"]
  );
});

function layeredTexture({
  slot = 1, width = 8, height = 8, layers = 12, mips = 4,
  dimension = 1, flags = 1, format = 1, usage = 6,
} = {}) {
  const payload = new Uint8Array(flags & 1 ? 28 : 24);
  const view = new DataView(payload.buffer);
  view.setUint32(0, handle(slot), true);
  view.setUint32(4, width, true);
  view.setUint32(8, height, true);
  view.setUint16(12, mips, true);
  view.setUint16(14, 1, true);
  view.setUint16(16, format, true);
  view.setUint8(18, dimension);
  view.setUint8(19, flags);
  view.setUint32(20, usage, true);
  if (flags & 1) view.setUint32(24, layers, true);
  return [3, payload];
}

function layeredView({
  dimension = 2, mip = 0, mips = 1, layer = 0, layers = 12, format = 1,
} = {}) {
  const payload = new Uint8Array(20);
  const view = new DataView(payload.buffer);
  view.setUint32(0, handle(2), true);
  view.setUint32(4, handle(1), true);
  view.setUint16(8, format, true);
  view.setUint8(10, dimension);
  view.setUint8(11, 1);
  view.setUint16(12, mip, true);
  view.setUint16(14, mips, true);
  view.setUint16(16, layer, true);
  view.setUint16(18, layers, true);
  return [23, payload];
}

function layeredUpload({
  mip = 1, z = 0, width = 4, height = 4, layers = 2,
  bytesPerRow = 20, rowsPerImage = 5, length = 176,
} = {}) {
  const payload = new Uint8Array(44 + Math.ceil(length / 4) * 4);
  const view = new DataView(payload.buffer);
  [handle(1), mip, 0, 0, z, width, height, layers, bytesPerRow, rowsPerImage, length]
    .forEach((value, index) => view.setUint32(index * 4, value, true));
  return [4, payload];
}

async function rejectsTextureBatch(items, diagnostic, index) {
  const engine = validationEngine();
  const rejected = [];
  engine.device = {
    pushErrorScope() { assert.fail("invalid texture batch reached the GPU"); },
  };
  engine.emitBatchRejected = (...args) => rejected.push(args);
  await engine.execute(commands(items));
  assert.equal(rejected.length, 1);
  assert.match(rejected[0][3], diagnostic);
  if (index !== undefined) assert.equal(rejected[0][0], index);
  assert.equal(engine.resources.size, 0);
  assert.equal(engine.handleSlots.size, 0);
  assert.equal(engine.lastSequence, 0n);
}

test("rejects unsupported texture shapes before GPU execution", async t => {
  for (const [name, descriptor] of [
    ["unknown flags", { flags: 3 }],
    ["unknown dimension", { dimension: 3 }],
    ["volume missing depth flag", { dimension: 2, flags: 0 }],
    ["zero layers", { layers: 0 }],
    ["too many layers", { layers: 257 }],
    ["volume width exceeds limit", { dimension: 2, width: 257 }],
    ["volume height exceeds limit", { dimension: 2, height: 257 }],
    ["volume depth exceeds limit", { dimension: 2, layers: 257 }],
    ["too many mips for shape", { mips: 5 }],
    ["volume depth format", { dimension: 2, format: 6 }],
    ["volume attachment", { dimension: 2, usage: 0x14 }],
    ["unknown usage", { usage: 0x20 }],
  ]) {
    await t.test(name, () => rejectsTextureBatch(
      [layeredTexture(descriptor)], /texture/,
    ));
  }
});

test("rejects incompatible texture view dimensions and ranges", async t => {
  for (const [name, texture, view] of [
    ["array from volume", { dimension: 2 }, {}],
    ["volume from array", {}, { dimension: 5, layers: 0 }],
    ["volume layer count", { dimension: 2 }, { dimension: 5, layers: 1 }],
    ["volume base layer", { dimension: 2 }, { dimension: 5, layers: 0, layer: 1 }],
    ["2d multiple layers", {}, { dimension: 1, layers: 2 }],
    ["cube face count", {}, { dimension: 3, layers: 5 }],
    ["cube array face count", {}, { dimension: 4, layers: 7 }],
    ["cube array empty", {}, { dimension: 4, layers: 0 }],
    ["cube nonsquare mip", { height: 4 }, { dimension: 3, layers: 6 }],
    ["cube nonsquare base despite square last mip", { width: 2, height: 1, mips: 2 },
      { dimension: 3, layers: 6, mip: 1 }],
    ["layer overflow", {}, { layer: 1 }],
    ["mip overflow", {}, { mip: 3, mips: 2 }],
    ["view format mismatch", {}, { format: 7 }],
  ]) {
    await t.test(name, () => rejectsTextureBatch(
      [layeredTexture(texture), layeredView(view)], /texture view/, 1,
    ));
  }
});

test("checks all images, padding and shrinking volume mip depth on upload", async t => {
  for (const [name, texture, upload, diagnostic] of [
    ["short last row", {}, { length: 175 }, /too short/],
    ["short intermediate image", {}, { length: 100 }, /too short/],
    ["short row stride", {}, { bytesPerRow: 15 }, /layout/],
    ["short image stride", {}, { rowsPerImage: 3 }, /layout/],
    ["array layer overflow", {}, { z: 11 }, /range/],
    ["volume mip depth overflow", { dimension: 2, layers: 2 }, {}, /range/],
    ["copy destination missing", { usage: 4 }, {}, /invalid texture upload/],
    ["integer overflow", {}, { bytesPerRow: 0xfffffffc, rowsPerImage: 0xffffffff }, /too short/],
  ]) {
    await t.test(name, () => rejectsTextureBatch(
      [layeredTexture(texture), layeredUpload(upload)], diagnostic, 1,
    ));
  }
  // The complete first image is padded; the last image ends at its last texel.
  const engine = validationEngine();
  const valid = engine.validate(parseCommands(commands([
    layeredTexture(), layeredUpload(),
  ])));
  engine.resources = valid.shadow;
  engine.handleSlots = valid.slots;
  // A later invalid upload leaves the already-created texture live.
  assert.throws(() => engine.validate(parseCommands(commands([
    layeredUpload({ length: 175 }),
  ]))), /too short/);
  assert.equal(engine.resources.has(handle(1)), true);
});

function textureBindingCommands(dimension, sampleType = 1) {
  const layout = new Uint8Array(40);
  const descriptor = new DataView(layout.buffer);
  descriptor.setUint32(0, handle(3), true);
  descriptor.setUint32(4, 1, true);
  descriptor.setUint32(12, 2, true);
  descriptor.setUint16(16, 3, true);
  descriptor.setUint32(32, sampleType, true);
  descriptor.setUint32(36, dimension, true);
  const group = new Uint8Array(44);
  const entry = new DataView(group.buffer);
  entry.setUint32(0, handle(4), true);
  entry.setUint32(4, handle(3), true);
  entry.setUint32(8, 1, true);
  entry.setUint32(16, handle(2), true);
  entry.setUint16(20, 3, true);
  return [[7, layout], [9, group]];
}

test("rejects texture binding view and sample type mismatches in every new mode", async t => {
  for (const [name, dimension, layers, texture] of [
    ["array", 2, 12, {}],
    ["cube", 3, 6, {}],
    ["cube array", 4, 12, {}],
    ["volume", 5, 0, { dimension: 2 }],
  ]) {
    await t.test(`${name} dimension`, () => rejectsTextureBatch([
      layeredTexture(texture), layeredView({ dimension, layers }),
      ...textureBindingCommands(1),
    ], /incompatible texture binding/, 3));
    await t.test(`${name} sample type`, () => rejectsTextureBatch([
      layeredTexture(texture), layeredView({ dimension, layers }),
      ...textureBindingCommands(dimension, 3),
    ], /incompatible texture binding/, 3));
  }
});

test("charges every layer and shrinking volume mip against the texture quota", async () => {
  await rejectsTextureBatch([
    layeredTexture({ width: 1024, height: 1024, layers: 65, mips: 1 }),
  ], /texture allocation budget/, 0);
  // 256^3 + 128^3 texels = 72MiB per volume: three fit, the fourth fails.
  await rejectsTextureBatch(Array.from({ length: 4 }, (_, i) =>
    layeredTexture({ slot: i + 1, dimension: 2, width: 256, height: 256, layers: 256, mips: 2 }),
  ), /texture allocation budget/, 3);
});

function depthAttachment({ format = 8, aspect = 1 } = {}) {
  const [, view] = layeredView({ dimension: 1, format, layers: 1 });
  view[11] = aspect;
  return [
    layeredTexture({ format, usage: 0x14, flags: 0 }),
    [23, view],
  ];
}

function stencilRenderPass({ flags = 3 | 8 | 32, clearStencil } = {}) {
  const values = [0, handle(2), 1, flags, 0, 0, 0, 0, 0x3f800000];
  if (clearStencil !== undefined) values.push(clearStencil);
  return [12, u32s(values)];
}

test("stencil pass operations need a stencil attachment and one stencil byte", async t => {
  const engine = validationEngine();
  const valid = engine.validate(parseCommands(commands([
    ...depthAttachment(),
    stencilRenderPass({ flags: 3 | 8 | 32 | 64, clearStencil: 255 }),
    [21, new Uint8Array()],
  ])));
  assert.equal(valid.commands[2].clearStencil, 255);
  for (const [name, attachment, pass] of [
    ["stencil flags on a depth-only attachment", { format: 5 }, { flags: 16 }],
    ["load and clear together", {}, { flags: 16 | 64, clearStencil: 1 }],
    ["clear value above one byte", {}, { flags: 64, clearStencil: 256 }],
  ]) {
    await t.test(name, () => rejectsTextureBatch(
      [...depthAttachment(attachment), stencilRenderPass(pass)], /stencil pass/, 2,
    ));
  }
  await t.test("depth-only aspect as attachment", () => rejectsTextureBatch(
    [...depthAttachment({ aspect: 2 }), stencilRenderPass()], /render attachment/, 2,
  ));
  await t.test("combined aspect as sampled view", () => rejectsTextureBatch([
    layeredTexture({ format: 8, usage: 0x14, flags: 0 }),
    layeredView({ dimension: 1, format: 8, layers: 1 }),
    ...textureBindingCommands(1, 3),
  ], /incompatible texture binding/, 3));
});

test("depth-only stencil views derive their aspect format before sampling", async () => {
  const engine = validationEngine();
  let viewDescriptor;
  let boundView;
  const textureView = {};
  engine.device = {
    pushErrorScope() {},
    popErrorScope: async () => null,
    createTexture: () => ({
      createView(descriptor) {
        viewDescriptor = descriptor;
        return textureView;
      },
    }),
    createBindGroupLayout: () => ({}),
    createBindGroup({ entries }) {
      boundView = entries[0].resource;
      return {};
    },
  };
  engine.emitBatchRejected = (...args) => assert.fail(args[3]);
  await engine.execute(commands([
    ...depthAttachment({ aspect: 2 }),
    ...textureBindingCommands(1, 3),
  ]));
  assert.equal(viewDescriptor.aspect, "depth-only");
  assert.equal(viewDescriptor.format, undefined,
    "WebGPU must derive depth24plus, not receive the combined depth24plus-stencil8 format");
  assert.equal(boundView, textureView);
  assert.equal(engine.lastSequence, 1);
});

function f32s(values) {
  const bytes = new Uint8Array(values.length * 4);
  const view = new DataView(bytes.buffer);
  values.forEach((value, index) => view.setFloat32(index * 4, value, true));
  return bytes;
}

test("blend constant applies in order inside its render pass only", async t => {
  const engine = validationEngine();
  const calls = [];
  Object.assign(engine, {
    context: { getCurrentTexture: () => ({ createView: () => ({}) }) },
    device: {
      pushErrorScope() {},
      popErrorScope: async () => null,
      createCommandEncoder: () => ({
        beginRenderPass() {
          calls.push(["begin"]);
          return {
            setBlendConstant(color) { calls.push(["blend", { ...color }]); },
            end() { calls.push(["end"]); },
          };
        },
        finish: () => ({}),
      }),
      queue: { submit() {}, onSubmittedWorkDone: async () => {} },
    },
    emitBatchRejected() {
      assert.fail("valid blend constant batch was rejected");
    },
  });
  const [begin, end] = renderPass(0);
  await engine.execute(commands([
    begin, [31, f32s([0.25, 0.5, 0.75, 1])], [31, f32s([1, 0, 0, 0.5])], end,
    begin, end,
  ]));
  assert.deepEqual(calls, [
    ["begin"],
    ["blend", { r: 0.25, g: 0.5, b: 0.75, a: 1 }],
    ["blend", { r: 1, g: 0, b: 0, a: 0.5 }],
    ["end"],
    ["begin"],
    ["end"],
  ]);

  for (const [name, items, diagnostic, index] of [
    ["outside a render pass", [[31, f32s([0, 0, 0, 0])]], /blend constant outside render pass/, 0],
    ["after the pass ended", [begin, end, [31, f32s([0, 0, 0, 0])]], /blend constant outside render pass/, 2],
    ["non-finite component", [begin, [31, f32s([0, Number.NaN, 0, 0])], end], /non-finite/],
    ["infinite component", [begin, [31, f32s([0, 0, 0, Infinity])], end], /non-finite/],
    ["three components", [begin, [31, f32s([0, 0, 0])], end], /GPU/],
  ]) {
    await t.test(name, () => rejectsTextureBatch(items, diagnostic, index));
  }
});

function occlusionPass(count, token = 0) {
  return [12, u32s([0, 0, 1, 2 | 128, 0, 0, 0, 0x3f800000, 0x3f800000, count, token])];
}

function occlusionEngine() {
  const engine = validationEngine();
  const gates = [];
  const passCalls = [];
  const buffers = [];
  const bytes = buffer => new Uint8Array(buffer.storage);
  Object.assign(engine, {
    occlusionDelivery: Promise.resolve(),
    occlusionEpoch: 0,
    context: { getCurrentTexture: () => ({ createView: () => ({}) }) },
    device: {
      pushErrorScope() {},
      popErrorScope: async () => null,
      createQuerySet: ({ count }) => ({ samples: new Array(count).fill(0n), destroy() {} }),
      createBuffer: ({ size }) => {
        const buffer = {
          storage: new ArrayBuffer(size),
          destroyed: false,
          mapAsync() {
            return new Promise(resolvePromise => gates.push(resolvePromise));
          },
          getMappedRange() { return this.storage; },
          destroy() { this.destroyed = true; },
        };
        buffers.push(buffer);
        return buffer;
      },
      createCommandEncoder: () => ({
        beginRenderPass({ occlusionQuerySet }) {
          let open = null;
          return {
            beginOcclusionQuery(query) { open = query; passCalls.push(["begin", query]); },
            endOcclusionQuery() { open = null; passCalls.push(["end"]); },
            setPipeline() {},
            draw(vertices) { occlusionQuerySet.samples[open] += BigInt(vertices); },
            end() { passCalls.push(["pass-end"]); },
          };
        },
        resolveQuerySet(querySet, first, count, destination) {
          querySet.samples.forEach((value, index) =>
            new DataView(destination.storage).setBigUint64(index * 8, value, true));
        },
        copyBufferToBuffer(source, sourceOffset, destination, destinationOffset, size) {
          bytes(destination).set(bytes(source).subarray(sourceOffset, sourceOffset + size), destinationOffset);
        },
        finish: () => ({}),
      }),
      queue: { submit() {}, onSubmittedWorkDone: async () => {} },
      destroy() {},
    },
    emitBatchRejected(...args) {
      assert.fail(`valid occlusion batch was rejected: ${args[3]}`);
    },
  });
  return { engine, gates, passCalls, buffers };
}

function decodeOcclusion(message) {
  const view = new DataView(message.bytes.buffer);
  const count = view.getUint32(28, true);
  return {
    type: view.getUint16(6, true),
    sequence: Number(view.getBigUint64(16, true)),
    token: view.getUint32(24, true),
    samples: Array.from({ length: count }, (_, index) =>
      Number(view.getBigUint64(32 + index * 8, true))),
  };
}

test("occlusion results follow their batch in submission order without blocking it", async () => {
  const { engine, gates, passCalls } = occlusionEngine();
  const capture = captureMessages();
  try {
    const [, end] = renderPass(0);
    const draw = vertices => [19, u32s([vertices, 1, 0, 0])];
    await engine.execute(commands([
      occlusionPass(3, 0xabcd),
      [32, u32s([0])], draw(3), [33, new Uint8Array()],
      [32, u32s([2])], draw(6), [33, new Uint8Array()],
      end,
    ], 1n));
    await engine.execute(commands([
      occlusionPass(1, 7), [32, u32s([0])], draw(9), [33, new Uint8Array()], end,
    ], 2n));
    const events = () => capture.messages
      .filter(message => message.type === "event")
      .map(message => new DataView(message.bytes.buffer).getUint16(6, true));
    assert.deepEqual(events(), [5, 5], "batches complete before any result is mapped");
    assert.deepEqual(passCalls.slice(0, 6), [
      ["begin", 0], ["end"], ["begin", 2], ["end"], ["begin", 1], ["end"],
    ], "the unused index runs an empty query before the pass ends");
    for (let index = 0; index < 2; index++) {
      while (gates.length <= index) await new Promise(setImmediate);
      gates[index]();
    }
    await engine.occlusionDelivery;
    const results = capture.messages
      .filter(message => message.type === "event" &&
        new DataView(message.bytes.buffer).getUint16(6, true) === 9)
      .map(decodeOcclusion);
    assert.deepEqual(results, [
      { type: 9, sequence: 1, token: 0xabcd, samples: [3, 0, 6] },
      { type: 9, sequence: 2, token: 7, samples: [9] },
    ]);
  } finally {
    capture.restore();
  }
});

test("in-flight occlusion results have bounded query and pass counts", async t => {
  for (const [name, count, passes] of [["queries", 4096, 1], ["passes", 1, 16]]) {
    await t.test(name, async () => {
      const { engine, buffers, gates } = occlusionEngine();
      const [, end] = renderPass(0);
      const items = Array.from({ length: passes }, () => [occlusionPass(count), end]).flat();
      for (let sequence = 1n; sequence <= 4n; sequence++) {
        await engine.execute(commands(items, sequence));
      }
      const allocated = buffers.length;
      const rejected = [];
      engine.emitBatchRejected = (...args) => rejected.push(args);
      await engine.execute(commands([occlusionPass(1), end], 5n));
      assert.equal(buffers.length, allocated, "reject before allocating more GPU readbacks");
      assert.equal(rejected.length, 1);
      assert.equal(rejected[0][1], 3);
      assert.match(rejected[0][3], /readback queue is full/);
      assert.equal(engine.lastSequence, 4);
      gates[0]();
      await new Promise(setImmediate);
      await engine.execute(commands([occlusionPass(1), end], 5n));
      assert.equal(rejected.length, 1, "completed results release quota for later submissions");
      engine.stop();
      assert.equal(engine.occlusionReadbacks.size, 0);
      assert.ok(buffers.every(buffer => buffer.destroyed));
    });
  }
});

test("reset destroys pending query buffers and fresh results bypass an abandoned map", async () => {
  const { engine, gates, buffers } = occlusionEngine();
  const capture = captureMessages();
  try {
    const [, end] = renderPass(0);
    await engine.execute(commands([occlusionPass(1, 10), end]));
    const oldDelivery = engine.occlusionDelivery;
    engine.reset();
    await engine.queue;
    assert.ok(buffers.every(buffer => buffer.destroyed), "reset releases results before the map settles");
    assert.equal(engine.occlusionReadbacks.size, 0);
    await engine.execute(commands([occlusionPass(1, 20), end]));
    assert.equal(gates.length, 2, "the fresh map starts despite the unresolved old map");
    gates[1]();
    await engine.occlusionDelivery;
    gates[0]();
    await oldDelivery;
    const results = capture.messages.filter(message =>
      message.type === "event" && eventType(message.bytes) === 9);
    assert.deepEqual(results.map(decodeOcclusion), [
      { type: 9, sequence: 1, token: 20, samples: [0] },
    ]);
    assert.ok(buffers.every(buffer => buffer.destroyed));
  } finally {
    engine.stop();
    capture.restore();
  }
});

test("backend rejection releases all query resources without emitting results", async () => {
  const { engine, buffers } = occlusionEngine();
  let scopes = 0;
  engine.device.popErrorScope = async () => ++scopes === 2 ? { message: "invalid pass" } : null;
  const rejected = [];
  engine.emitBatchRejected = (...args) => rejected.push(args);
  const [, end] = renderPass(0);
  await engine.execute(commands([occlusionPass(1), end]));
  assert.equal(rejected.length, 1);
  assert.match(rejected[0][3], /invalid pass/);
  assert.equal(engine.occlusionReadbacks.size, 0);
  assert.ok(buffers.every(buffer => buffer.destroyed));
  assert.equal(engine.lastSequence, 0n);
  engine.stop();
});

test("device restoration fences old batches waiting for backend validation", async () => {
  const { engine, buffers } = occlusionEngine();
  const replacement = occlusionEngine().engine;
  const scopes = [];
  engine.device.popErrorScope = () => new Promise(resolveScope => scopes.push(resolveScope));
  engine.configureSurface = () => {};
  engine.observeDevice = () => {};
  engine.capabilities = () => new Uint8Array();
  engine.deviceGeneration = 1;
  const acquire = GpuEngine.acquireDevice;
  GpuEngine.acquireDevice = async () => ({
    device: replacement.device,
    context: replacement.context,
    format: "rgba8unorm",
    limits: replacement.limits,
  });
  const capture = captureMessages();
  try {
    const [, end] = renderPass(0);
    engine.submit(commands([
      [1, u32s([handle(9), 8, 4, 0])], occlusionPass(1), end,
    ]));
    engine.reset();
    const oldQueue = engine.queue;
    await Promise.resolve();
    assert.equal(scopes.length, 2);
    engine.stopped = true;
    engine.abandonOcclusionResults();
    await engine.restore();
    await engine.execute(commands([[1, u32s([handle(10), 8, 4, 0])]]));
    scopes.forEach(resolveScope => resolveScope(null));
    await oldQueue;
    assert.deepEqual([...engine.resources.keys()], [handle(10)],
      "old commits and queued resets cannot overwrite the replacement catalog");
    assert.equal(engine.lastSequence, 1);
    assert.equal(engine.pendingBatches, 0, "old finalizers cannot decrement the replacement queue");
    assert.equal(engine.stopped, false);
    assert.ok(buffers.every(buffer => buffer.destroyed));
    assert.deepEqual(capture.messages.filter(message => message.type === "event")
      .map(message => eventType(message.bytes)), [8, 5], "only the replacement batch completes");
  } finally {
    GpuEngine.acquireDevice = acquire;
    engine.stop();
    capture.restore();
  }
});

test("device loss and fatal batches immediately release pending occlusion buffers", async t => {
  for (const failure of ["device loss", "malformed batch"]) {
    await t.test(failure, async () => {
      const { engine, buffers, gates } = occlusionEngine();
      let loseDevice;
      engine.device.addEventListener = () => {};
      engine.device.lost = new Promise(resolveLoss => { loseDevice = resolveLoss; });
      engine.restore = async () => {};
      engine.observeDevice(engine.device);
      const capture = captureMessages();
      try {
        const [, end] = renderPass(0);
        await engine.execute(commands([occlusionPass(1), end]));
        const oldDelivery = engine.occlusionDelivery;
        if (failure === "device loss") {
          loseDevice({ message: "test loss" });
          await Promise.resolve();
        } else {
          engine.submit(new Uint8Array());
          await engine.queue;
        }
        assert.equal(engine.stopped, true);
        assert.equal(engine.occlusionReadbacks.size, 0);
        assert.ok(buffers.every(buffer => buffer.destroyed));
        gates[0]();
        await oldDelivery;
        assert.equal(capture.messages.some(message =>
          message.type === "event" && eventType(message.bytes) === 9), false);
      } finally {
        engine.stop();
        capture.restore();
      }
    });
  }
});

test("reset discards shader diagnostics belonging to the old resource catalog", async () => {
  const engine = validationEngine();
  let finishCompilation;
  const entry = { value: { getCompilationInfo: () =>
    new Promise(resolveInfo => { finishCompilation = resolveInfo; }) } };
  const capture = captureMessages();
  try {
    const pending = engine.watchShader(entry, handle(1), 1);
    engine.reset();
    await engine.queue;
    finishCompilation({ messages: [{ type: "error", message: "old shader" }] });
    await pending;
    assert.equal(capture.messages.length, 0);
    assert.equal(entry.failed, undefined);
  } finally {
    capture.restore();
  }
});

test("occlusion queries follow WebGPU nesting and uniqueness rules", async t => {
  const [, end] = renderPass(0);
  const begin = query => [32, u32s([query])];
  const close = [33, new Uint8Array()];
  for (const [name, items, diagnostic] of [
    ["nested", [occlusionPass(2), begin(0), begin(1), close, close, end], /cannot nest/],
    ["index beyond count", [occlusionPass(2), begin(2), close, end], /exceeds the pass query count/],
    ["reused index", [occlusionPass(2), begin(0), close, begin(0), close, end], /reused/],
    ["end without begin", [occlusionPass(2), close, end], /not active/],
    ["open at pass end", [occlusionPass(2), begin(0), end], /open occlusion query/],
    ["pass without queries", [renderPass(0)[0], begin(0), close, end], /no occlusion queries/],
    ["outside a pass", [begin(0)], /outside render pass/],
    ["zero count", [occlusionPass(0), end], /must be nonzero/],
    ["batch limit", [occlusionPass(4096), end, occlusionPass(1), end], /batch limit/],
  ]) {
    await t.test(name, () => rejectsTextureBatch(items, diagnostic));
  }
});

function stencilPipeline({ depthFormat = 8, topology = 4, compare = 3, bias = 0 } = {}) {
  const payload = new Uint8Array(40 + 16 + 24);
  const view = new DataView(payload.buffer);
  view.setUint32(0, handle(5), true);
  view.setUint32(4, handle(3), true);
  view.setUint32(8, handle(1), true);
  view.setUint16(16, 1, true);
  view.setUint16(18, 2, true);
  view.setUint16(20, depthFormat, true);
  view.setUint16(22, 1, true);
  payload[24] = topology;
  payload[25] = 1;
  payload[28] = 8;
  view.setUint16(40, 1, true);
  view.setUint16(42, 15, true);
  payload.set([1, 2, 1, 1, 2, 1], 44);
  payload.set([compare, 1, 1, 3, compare, 1, 1, 3, 0xff, 0xff], 56);
  view.setInt32(68, bias, true);
  return [10, payload];
}

test("pipeline stencil and depth bias follow WebGPU's format and topology rules", async t => {
  const prerequisites = [shaderCommands(1)[0], [8, u32s([handle(3), 0])]]
    .map(item => (Array.isArray(item) ? item : [item.opcode, item.payload]));
  const engine = validationEngine();
  const valid = engine.validate(parseCommands(commands([
    ...prerequisites, stencilPipeline({ bias: -8 }),
  ])));
  assert.equal(valid.commands[2].stencilFront.compare, "equal");
  assert.equal(valid.commands[2].stencilFront.passOp, "replace");
  assert.equal(valid.commands[2].depthBias, -8);
  for (const [name, pipeline] of [
    ["stencil test without a stencil format", { depthFormat: 5 }],
    ["trailer without a depth format", { depthFormat: 0, compare: 8 }],
    ["depth bias on lines", { topology: 2, compare: 8, bias: 1 }],
  ]) {
    await t.test(name, () => rejectsTextureBatch(
      [...prerequisites, stencilPipeline(pipeline)], /stencil or depth bias/, 2,
    ));
  }
});

function replacementDevice(lost = new Promise(() => {})) {
  return {
    device: { addEventListener() {}, lost, destroy() {} },
    context: { configure() {} },
    format: "bgra8unorm",
    limits: Array.from({ length: 21 }, () => 2048),
  };
}

test("success followed by immediate device loss shares the retry ceiling and backoff", async () => {
  const engine = lostEngine();
  let attempts = 0;
  const delays = [];
  const acquire = GpuEngine.acquireDevice;
  const setTimeout = context.setTimeout;
  GpuEngine.acquireDevice = async () => {
    attempts++;
    // Stop the unfixed implementation eventually rather than hanging the test.
    return replacementDevice(
      attempts < 12
        ? Promise.resolve({ message: "replacement immediately lost" })
        : new Promise(() => {}),
    );
  };
  context.setTimeout = (callback, delay) => {
    delays.push(delay);
    queueMicrotask(callback);
  };
  const capture = captureMessages();
  try {
    await engine.restore();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(attempts, 3);
    assert.deepEqual(delays, [250, 500]);
    assert.equal(engine.stopped, true);
    assert.equal(capture.messages.filter(message => message.type === "capabilities").length, 3);
    assert.equal(capture.messages.filter(message => message.type === "error").length, 1);
    await engine.restore();
    assert.equal(attempts, 3, "a failed episode cannot restart itself");
    assert.equal(capture.messages.filter(message => message.type === "error").length, 1);
  } finally {
    engine.stop();
    GpuEngine.acquireDevice = acquire;
    context.setTimeout = setTimeout;
    capture.restore();
  }
});

test("healthy intervals reset the recovery budget instead of imposing a lifetime ceiling", async () => {
  const engine = lostEngine();
  let attempts = 0;
  let now = 0;
  const delays = [];
  const acquire = GpuEngine.acquireDevice;
  const setTimeout = context.setTimeout;
  const clock = context.performance.now;
  context.performance.now = () => now;
  context.setTimeout = (callback, delay) => {
    delays.push(delay);
    queueMicrotask(callback);
  };
  GpuEngine.acquireDevice = async () => {
    attempts++;
    return replacementDevice();
  };
  const capture = captureMessages();
  try {
    for (let episode = 0; episode < 6; episode++) {
      engine.stopped = true;
      await engine.restore();
      assert.equal(engine.stopped, false);
      assert.equal(engine.restoreAttempts, 1);
      now += 30_000;
    }
    assert.equal(attempts, 6);
    assert.deepEqual(delays, []);
    assert.equal(capture.messages.some(message => message.type === "error"), false);
  } finally {
    engine.stop();
    GpuEngine.acquireDevice = acquire;
    context.setTimeout = setTimeout;
    context.performance.now = clock;
    capture.restore();
  }
});

test("acquisition failures and short-lived replacements share one recovery budget", async () => {
  const engine = lostEngine();
  let attempts = 0;
  const acquire = GpuEngine.acquireDevice;
  GpuEngine.acquireDevice = async () => {
    attempts++;
    if (attempts === 1 || attempts > 3) {
      throw new Error("adapter unavailable");
    }
    return replacementDevice(Promise.resolve({ message: "lost again" }));
  };
  const capture = captureMessages();
  try {
    await engine.restore();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(attempts, 3);
    assert.equal(capture.messages.filter(message => message.type === "capabilities").length, 2);
    assert.equal(capture.messages.filter(message => message.type === "error").length, 1);
  } finally {
    engine.stop();
    GpuEngine.acquireDevice = acquire;
    capture.restore();
  }
});

test("disposal during backoff cancels further acquisition and publication", async () => {
  const engine = lostEngine();
  let attempts = 0;
  let resume;
  const acquire = GpuEngine.acquireDevice;
  const setTimeout = context.setTimeout;
  GpuEngine.acquireDevice = async () => {
    attempts++;
    throw new Error("adapter unavailable");
  };
  context.setTimeout = callback => { resume = callback; };
  const capture = captureMessages();
  try {
    const restoring = engine.restore();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(typeof resume, "function");
    engine.stop();
    resume();
    await restoring;
    assert.equal(attempts, 1);
    assert.deepEqual(capture.messages, []);
  } finally {
    GpuEngine.acquireDevice = acquire;
    context.setTimeout = setTimeout;
    capture.restore();
  }
});

for (const supersede of [false, true]) {
  test(`an acquisition completed after ${supersede ? "device replacement" : "disposal"} is destroyed without publication`, async () => {
    const engine = lostEngine();
    let resolveAcquisition;
    let destroyed = 0;
    let attempts = 0;
    const replacement = replacementDevice();
    replacement.device.destroy = () => { destroyed++; };
    const acquire = GpuEngine.acquireDevice;
    GpuEngine.acquireDevice = () => {
      attempts++;
      return new Promise(resolve => { resolveAcquisition = resolve; });
    };
    const capture = captureMessages();
    try {
      const restoring = engine.restore();
      await engine.restore();
      assert.equal(attempts, 1, "recovery is single-flight");
      if (supersede) {
        engine.device = replacementDevice().device;
      } else {
        engine.stop();
      }
      resolveAcquisition(replacement);
      await restoring;
      assert.equal(destroyed, 1);
      assert.equal(engine.deviceGeneration, 1);
      assert.deepEqual(capture.messages, []);
    } finally {
      engine.stop();
      GpuEngine.acquireDevice = acquire;
      capture.restore();
    }
  });
}

test("loss from a superseded device cannot start another recovery", async () => {
  let loseOldDevice;
  const old = replacementDevice(new Promise(resolve => { loseOldDevice = resolve; }));
  const engine = lostEngine({ device: old.device });
  engine.observeDevice(old.device);
  let attempts = 0;
  const acquire = GpuEngine.acquireDevice;
  GpuEngine.acquireDevice = async () => {
    attempts++;
    return replacementDevice();
  };
  const capture = captureMessages();
  try {
    await engine.restore();
    capture.messages.length = 0;
    loseOldDevice({ message: "stale device loss" });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(attempts, 1);
    assert.equal(engine.stopped, false);
    assert.deepEqual(capture.messages, []);
  } finally {
    engine.stop();
    GpuEngine.acquireDevice = acquire;
    capture.restore();
  }
});
