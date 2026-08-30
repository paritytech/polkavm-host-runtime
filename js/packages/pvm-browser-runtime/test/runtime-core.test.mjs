import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

await import("../src/pvm-wasm-translated.js");
await import("../src/pvm-runtime-core.js");

const packageRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(packageRoot, "../../..");

function bytesBuffer(bytes) {
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  );
}

function endpoint() {
  const messages = [];
  const receiver = {
    onmessage: null,
    postMessage(message) {
      messages.push(message);
    },
  };
  globalThis.createPvmRuntime(receiver);
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
async function waitForStartupStage(
  messages,
  stage,
  timeoutMs = 10_000,
) {
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
  throw new Error(`timed out waiting for browser runtime startup stage ${stage}`);
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

test("compiler backend enforces the declared graphics profile", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/pvm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/pvm-runtime/tests/fixtures/framebuffer-test.polkavm",
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
  await new Promise((resolve) => setTimeout(resolve, 100));
  assert.equal(
    messages.some((message) => message.type === "frame"),
    false,
    "a framebuffer submission must not escape a tri2d declaration",
  );
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("compiler backend discards stale CoreVM mouse movement", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/pvm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/pvm-runtime/tests/fixtures/framebuffer-test.polkavm",
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

  const translated = new globalThis.TranslatedPvmRuntime(
    compiled.module,
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
});

test("browser runtime can select the interpreter without attempting translation", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/pvm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/pvm-runtime/tests/fixtures/framebuffer-test.polkavm",
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
  assert.deepEqual(
    messages
      .filter((message) => message.type === "startup")
      .map((message) => message.stage),
    [
      "runtime-instantiating",
      "runtime-instantiated",
      "interpreter-staging-program",
      "interpreter-program-staged",
      "interpreter-launch-begin",
      "interpreter-launch-begun",
      "interpreter-mounting-assets",
      "interpreter-assets-mounted",
      "interpreter-launch-starting",
      "interpreter-launch-started",
      "interpreter-initializing",
      "interpreter-initialized",
      "first-update-started",
      "first-update-completed",
    ],
  );

  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});
