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
