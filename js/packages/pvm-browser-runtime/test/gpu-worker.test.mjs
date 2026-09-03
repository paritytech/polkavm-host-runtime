import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(
  resolve(import.meta.dirname, "../src/pvm-gpu-worker.js"),
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
});
vm.runInContext(
  `${source}\nglobalThis.gpuWorkerTest = { parseCommand };`,
  context,
);
const { parseCommand } = context.gpuWorkerTest;

function parse(opcode, payload) {
  return parseCommand({ opcode, payload, index: 0 });
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
