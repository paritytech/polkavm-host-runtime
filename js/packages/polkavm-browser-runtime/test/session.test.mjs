/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Worker as NodeWorker } from "node:worker_threads";
import test from "node:test";
import { startSession } from "../src/session.js";

// Only adapt the browser Worker transport to Node. Execution, startup, binary
// cloning, errors, host-frame handling and cancellation use the shipped worker.
class BrowserWorker extends EventTarget {
  static active = new Set();
  static exits = [];
  constructor(url) {
    super();
    const source = `
      import { parentPort } from "node:worker_threads";
      globalThis.postMessage = (message, transfers) => parentPort.postMessage(message, transfers);
      await import(${JSON.stringify(String(url))});
      parentPort.on("message", (data) => globalThis.onmessage?.({ data }));
    `;
    this.worker = new NodeWorker(new URL(`data:text/javascript,${encodeURIComponent(source)}`));
    BrowserWorker.active.add(this);
    this.worker.on("message", (data) => this.dispatchEvent(new MessageEvent("message", { data })));
    this.worker.on("error", (error) => {
      const event = new Event("error", { cancelable: true });
      event.message = error.message;
      this.dispatchEvent(event);
    });
  }
  postMessage(message) { this.worker.postMessage(message); }
  terminate() {
    BrowserWorker.active.delete(this);
    BrowserWorker.exits.push(this.worker.terminate());
  }
}

async function options(onOutput = () => {}) {
  return {
    workerUrl: new URL("../dist/polkavm-worker.js", import.meta.url),
    runtime: await readFile(new URL("../dist/polkavm-browser-runtime.wasm", import.meta.url)),
    program: await readFile(new URL("../../../../rust/crates/polkavm-host-runtime/tests/fixtures/host-frame-roundtrip.polkavm", import.meta.url)),
    assets: [],
    graphicsProfile: "framebuffer",
    audioEnabled: false,
    onOutput,
  };
}

function installWorker(t) {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: BrowserWorker });
  t.after(async () => {
    if (previous) Object.defineProperty(globalThis, "Worker", previous);
    else delete globalThis.Worker;
    for (const worker of BrowserWorker.active) worker.terminate();
    await Promise.all(BrowserWorker.exits.splice(0));
  });
}

test("session startup failures reject ready, report the error and release the worker", { timeout: 20_000 }, async (t) => {
  installWorker(t);
  for (const failure of ["load", "runtime", "clone"]) {
    const startup = await options();
    if (failure === "load") startup.workerUrl = new URL("./missing-worker.js", import.meta.url);
    if (failure === "runtime") startup.runtime = new Uint8Array([0]);
    if (failure === "clone") startup.program = () => {};
    const session = startSession(startup);
    await assert.rejects(session.ready);
    const terminal = await session.terminal;
    assert.equal(terminal.reason, "error");
    assert.ok(terminal.error instanceof Error);
    assert.equal(session.state, "terminated");
    assert.equal(BrowserWorker.active.size, 0);
    assert.strictEqual(await session.stop(), terminal);
    assert.throws(() => session.send({ type: "pause", paused: false }), /terminated/);
  }
});

test("stop during startup rejects ready and cannot be resurrected", { timeout: 20_000 }, async (t) => {
  installWorker(t);
  const outputs = [];
  const session = startSession(await options((output) => outputs.push(output)));
  assert.throws(() => session.send({ type: "pause", paused: true }), /await ready/);
  const stopped = session.stop();
  assert.strictEqual(session.stop(), stopped);
  await assert.rejects(session.ready, { name: "AbortError" });
  assert.deepEqual(await stopped, { reason: "stopped" });
  assert.equal(session.state, "terminated");
  assert.equal(BrowserWorker.active.size, 0);
  assert.equal(outputs.some((output) => output.type === "ready"), false);
});

test("session host responses reach the real guest across a background transition", { timeout: 20_000 }, async (t) => {
  installWorker(t);
  const request = Promise.withResolvers();
  const saved = Promise.withResolvers();
  const session = startSession(await options((output) => {
    if (output.type === "host-frame-request") request.resolve(output.bytes);
    if (output.type === "save") saved.resolve(output.bytes);
  }));
  t.after(() => session.stop());
  const ready = await session.ready;
  assert.equal(ready.backend, "compiler");
  assert.equal(new TextDecoder().decode(await request.promise), "host-frame-conformance-request-v1");
  session.send({ type: "background", backgrounded: true, seq: 1 });
  session.send({ type: "host-frame-response", seq: 2,
    bytes: new TextEncoder().encode("host-frame-conformance-response-v1") });
  assert.equal(new TextDecoder().decode(await saved.promise), "host-frame-roundtrip-ok");
  assert.deepEqual(await session.stop(), { reason: "stopped" });
  assert.equal(BrowserWorker.active.size, 0);
});

test("runtime and host output-handler failures release a running session", { timeout: 20_000 }, async (t) => {
  installWorker(t);
  for (const failure of ["runtime", "host"]) {
    const session = startSession(await options((output) => {
      if (failure === "host" && output.type === "host-frame-request") {
        throw new Error("host presentation failed");
      }
    }));
    await session.ready;
    if (failure === "runtime") {
      session.send({ type: "view-insets", eventType: 16, left: -1, top: 0, right: 0, bottom: 0 });
    }
    const terminal = await session.terminal;
    assert.equal(terminal.reason, "error");
    assert.match(terminal.error.message, failure === "host" ? /host presentation failed/ : /invalid.*inset/);
    assert.equal(session.state, "terminated");
    assert.equal(BrowserWorker.active.size, 0);
  }
});

// Control protocol acknowledgments independently of worker scheduling so these
// tests can distinguish cleanup in progress from a forcibly terminated worker.
function installProtocolWorker(t) {
  const workers = [];
  const previous = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class ProtocolWorker extends EventTarget {
    messages = [];
    terminated = false;
    constructor() {
      super();
      workers.push(this);
    }
    postMessage(message) {
      this.messages.push(structuredClone(message));
    }
    emit(data) { this.dispatchEvent(new MessageEvent("message", { data })); }
    terminate() { this.terminated = true; }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: ProtocolWorker });
  t.after(() => {
    if (previous) Object.defineProperty(globalThis, "Worker", previous);
    else delete globalThis.Worker;
  });
  return workers;
}

test("recoverable cache errors preserve startup and running execution", async (t) => {
  const workers = installProtocolWorker(t);
  const outputs = [];
  const session = startSession({ fileCache: true, onOutput: (output) => outputs.push(output) });
  const worker = workers[0];
  worker.emit({ type: "error", message: "private cache unavailable", fatal: false });
  assert.equal(session.state, "starting");
  worker.emit({ type: "ready", backend: "interpreter" });
  await session.ready;
  worker.emit({ type: "error", message: "private cache deletion failed", fatal: false });
  session.send({ type: "pause", paused: false });
  assert.equal(session.state, "running");
  assert.equal(worker.terminated, false);
  assert.deepEqual(worker.messages.map(({ type }) => type), ["start", "pause"]);
  assert.deepEqual(outputs.map(({ type }) => type), ["error", "ready", "error"]);
  worker.emit({ type: "terminated", cleanupFailed: true });
  assert.deepEqual(await session.terminal, { reason: "terminated", cleanupFailed: true });
});

test("fatal errors await cleanup and preserve the first error through teardown", async (t) => {
  const workers = installProtocolWorker(t);
  const outputs = [];
  const session = startSession({ fileCache: true, onOutput: (output) => outputs.push(output) });
  const worker = workers[0];
  let settled = false;
  void session.terminal.then(() => { settled = true; });
  worker.emit({ type: "error", message: "guest initialization failed" });
  await assert.rejects(session.ready, /guest initialization failed/);
  assert.equal(session.state, "stopping");
  assert.equal(worker.terminated, false);
  assert.equal(settled, false);
  assert.deepEqual(worker.messages.map(({ type }) => type), ["start", "stop"]);
  worker.emit({ type: "ready", backend: "interpreter" });
  worker.emit({ type: "frame", pixels: new Uint8Array() });
  worker.emit({ type: "file-registrations", registrations: [] });
  worker.emit({ type: "mediated-input-cancel", handle: 1 });
  worker.emit({ type: "error", message: "secondary runtime error" });
  worker.emit({ type: "error", message: "cleanup trouble", fatal: false });
  assert.equal(worker.terminated, false);
  assert.equal(settled, false);
  worker.emit({ type: "terminated", cleanupFailed: true });
  const terminal = await session.terminal;
  assert.equal(terminal.reason, "error");
  assert.equal(terminal.error.message, "guest initialization failed");
  assert.equal(terminal.cleanupFailed, true);
  assert.equal(worker.terminated, true);
  assert.deepEqual(outputs.map(({ type }) => type), [
    "error", "file-registrations", "mediated-input-cancel", "error", "error", "terminated",
  ]);
  assert.strictEqual(await session.stop(), terminal);
});

test("throwing output observers are disabled while graceful cleanup finishes", async (t) => {
  const workers = installProtocolWorker(t);
  const failure = new Error("host output failed");
  let calls = 0;
  const session = startSession({ fileCache: true, onOutput: () => { calls++; throw failure; } });
  const worker = workers[0];
  worker.emit({ type: "ready", backend: "interpreter" });
  assert.equal((await session.ready).type, "ready");
  assert.equal(session.state, "stopping");
  assert.equal(worker.terminated, false);
  worker.emit({ type: "mediated-input-cancel", handle: 1 });
  worker.emit({ type: "error", message: "cleanup error", fatal: false });
  worker.emit({ type: "terminated" });
  assert.deepEqual(await session.terminal, { reason: "error", error: failure });
  assert.equal(calls, 1);
  assert.equal(worker.terminated, true);
});

test("a fatal runtime error takes precedence over its observer throwing", async (t) => {
  const workers = installProtocolWorker(t);
  const session = startSession({ onOutput: () => { throw new Error("observer failed"); } });
  const worker = workers[0];
  worker.emit({ type: "error", message: "runtime failed first" });
  await assert.rejects(session.ready, /runtime failed first/);
  worker.emit({ type: "terminated" });
  assert.equal((await session.terminal).error.message, "runtime failed first");
});

test("an observer throwing on terminated preserves its cleanup acknowledgment", async (t) => {
  const workers = installProtocolWorker(t);
  const failure = new Error("terminal observer failed");
  const session = startSession({
    fileCache: true,
    onOutput: () => { throw failure; },
  });
  workers[0].emit({ type: "terminated" });
  assert.deepEqual(await session.terminal, { reason: "error", error: failure });
  assert.deepEqual(workers[0].messages.map(({ type }) => type), ["start"]);
});

test("send clone failures request cleanup without transferring binary ownership", async (t) => {
  const workers = installProtocolWorker(t);
  const bytes = new Uint8Array([1, 2, 3]);
  const session = startSession({
    fileCache: true,
    fileRelaunch: { id: "document", mountPath: "selected.txt", name: "selected.txt",
      mimeType: "text/plain", bytes },
    onOutput() {},
  });
  const worker = workers[0];
  worker.emit({ type: "ready", backend: "interpreter" });
  await session.ready;
  session.send({ type: "file-input", handle: 1, name: "a.txt", mimeType: "text/plain", bytes });
  assert.notStrictEqual(worker.messages[0].fileRelaunch.bytes.buffer, bytes.buffer);
  assert.notStrictEqual(worker.messages[1].bytes.buffer, bytes.buffer);
  assert.deepEqual(bytes, new Uint8Array([1, 2, 3]));
  assert.throws(() => session.send({ type: "input", bytes: () => {} }), { name: "DataCloneError" });
  assert.equal(session.state, "stopping");
  assert.equal(worker.terminated, false);
  assert.equal(worker.messages.at(-1).type, "stop");
  worker.emit({ type: "terminated" });
  const terminal = await session.terminal;
  assert.equal(terminal.reason, "error");
  assert.equal(terminal.error.name, "DataCloneError");
  assert.equal(terminal.cleanupFailed, undefined);
});

test("forced stop marks private-cache cleanup unconfirmed after the bounded guard", async (t) => {
  const workers = installProtocolWorker(t);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const fileCache of [false, true]) {
    const session = startSession({ fileCache, onOutput() {} });
    const worker = workers.at(-1);
    const stopped = session.stop();
    assert.strictEqual(session.stop(), stopped);
    await assert.rejects(session.ready, { name: "AbortError" });
    t.mock.timers.tick(999);
    assert.equal(worker.terminated, false);
    t.mock.timers.tick(1);
    assert.deepEqual(await stopped, {
      reason: "stopped", ...(fileCache ? { cleanupFailed: true } : {}),
    });
    assert.equal(worker.terminated, true);
  }
});

test("fatal cleanup timeout retains the error and reports unconfirmed deletion", async (t) => {
  const workers = installProtocolWorker(t);
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const session = startSession({ fileCache: true, onOutput() {} });
  workers[0].emit({ type: "error", message: "guest failure" });
  t.mock.timers.tick(1000);
  const terminal = await session.terminal;
  assert.equal(terminal.reason, "error");
  assert.equal(terminal.error.message, "guest failure");
  assert.equal(terminal.cleanupFailed, true);
});

test("worker faults and decoding failures cannot confirm private-cache deletion", async (t) => {
  const workers = installProtocolWorker(t);
  for (const type of ["error", "messageerror"]) {
    const session = startSession({ fileCache: true, onOutput() {} });
    const worker = workers.at(-1);
    const event = new Event(type, { cancelable: true });
    event.message = "worker fault";
    worker.dispatchEvent(event);
    const terminal = await session.terminal;
    assert.equal(terminal.reason, "error");
    assert.match(terminal.error.message, type === "error" ? /worker fault/ : /could not be decoded/);
    assert.equal(terminal.cleanupFailed, true);
    assert.equal(worker.terminated, true);
    if (type === "error") assert.equal(event.defaultPrevented, true);
  }
});

test("failed stop posting reports unconfirmed cleanup and preserves the initial failure", async (t) => {
  const workers = installProtocolWorker(t);
  const session = startSession({ fileCache: true, onOutput() {} });
  const worker = workers[0];
  worker.postMessage = () => { throw new Error("worker transport unavailable"); };
  worker.emit({ type: "error", message: "initial guest failure" });
  const terminal = await session.terminal;
  assert.equal(terminal.reason, "error");
  assert.equal(terminal.error.message, "initial guest failure");
  assert.equal(terminal.cleanupFailed, true);
  assert.equal(worker.terminated, true);
});

test("ready observers can send immediately and stop filters late ready/frame output", async (t) => {
  const workers = installProtocolWorker(t);
  const outputs = [];
  const session = startSession({ fileCache: true, onOutput(output) {
    outputs.push(output);
    if (output.type === "ready") {
      assert.equal(session.state, "running");
      session.send({ type: "pause", paused: false });
    }
  } });
  const worker = workers[0];
  worker.emit({ type: "ready", backend: "interpreter" });
  await session.ready;
  const stopped = session.stop();
  worker.emit({ type: "ready", backend: "interpreter" });
  worker.emit({ type: "frame", pixels: new Uint8Array() });
  worker.emit({ type: "terminated" });
  assert.deepEqual(await stopped, { reason: "stopped" });
  assert.deepEqual(outputs.map(({ type }) => type), ["ready", "terminated"]);
  assert.deepEqual(worker.messages.map(({ type }) => type), ["start", "pause", "stop"]);
  worker.emit({ type: "error", message: "late ignored error" });
  assert.equal(outputs.length, 2);
});

test("acknowledged stop propagates cleanup failure without waiting for the guard", async (t) => {
  const workers = installProtocolWorker(t);
  const session = startSession({ fileCache: true, onOutput() {} });
  const stopped = session.stop();
  workers[0].emit({ type: "terminated", cleanupFailed: true });
  assert.deepEqual(await stopped, { reason: "stopped", cleanupFailed: true });
  assert.equal(workers[0].terminated, true);
});

test("startup post failures still request cleanup from the usable worker", async (t) => {
  const workers = installProtocolWorker(t);
  const session = startSession({ fileCache: true, program: () => {}, onOutput() {} });
  await assert.rejects(session.ready, { name: "DataCloneError" });
  const worker = workers[0];
  assert.equal(session.state, "stopping");
  assert.equal(worker.terminated, false);
  assert.deepEqual(worker.messages, [{ type: "stop" }]);
  worker.emit({ type: "terminated" });
  const terminal = await session.terminal;
  assert.equal(terminal.reason, "error");
  assert.equal(terminal.error.name, "DataCloneError");
  assert.equal(terminal.cleanupFailed, undefined);
});
