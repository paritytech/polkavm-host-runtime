import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  closeSync, existsSync, fstatSync, fsyncSync, ftruncateSync, mkdtempSync, openSync, readSync,
  renameSync, rmSync, writeSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";

await import("../src/polkavm-wasm-translated.js");
await import("../src/polkavm-runtime-core.js");

// The fixture and its probe layout are described in
// rust/crates/polkavm-host-runtime/tests/file_input.rs.
const packageRoot = resolve(import.meta.dirname, "..");
const fixtures = resolve(
  packageRoot,
  "../../../rust/crates/polkavm-host-runtime/tests/fixtures",
);
const runtime = await readFile(
  resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
);
const program = await readFile(resolve(fixtures, "file-input.polkavm"));
const streamProgram = await readFile(resolve(fixtures, "file-stream.polkavm"));
const vectors = JSON.parse(
  await readFile(resolve(fixtures, "file-descriptors.json"), "utf8"),
);
const encoder = new TextEncoder();
const decoder = new TextDecoder();

const MOUNT_PATH = "game/cartridge.bin";
const ENTRYPOINT = "app.polkavm";
const INFO_BYTES = 128;
const DATA_BYTES = 64;
const PROBE_STRIDE = 12 + INFO_BYTES + DATA_BYTES;
const ASSET_OFFSET = PROBE_STRIDE * 4;
const CANCEL_OFFSET = ASSET_OFFSET + 4 + DATA_BYTES;
const HEADER_BYTES = 16 * 4 + 4;
const INLINE =
  '{"id":"doc","label":"Text document","extensions":[".txt"],"mimeTypes":["text/plain"],"delivery":"inline","maxBytes":16}';
const RELAUNCH =
  '{"id":"rom","label":"Cartridge","extensions":[".bin"],"delivery":"relaunch","maxBytes":32,"mountPath":"game/cartridge.bin"}';
const STREAM =
  '{"id":"stream","label":"Stream file","extensions":[".bin"],"delivery":"stream","maxBytes":4294967295}';
const BOTH = { inline: true, relaunch: true, stream: true, entrypoint: ENTRYPOINT };
const BACKENDS = [false, true];
const compiledPrograms = new Map();

// Node has no FileReaderSync. This test reader performs real bounded disk I/O;
// Chromium exercises the platform Blob/FileReaderSync implementation separately.
class DiskFileReaderSync {
  readAsArrayBuffer({ disk, offset, length }) {
    const bytes = new Uint8Array(length);
    disk.reads.push([offset, length]);
    const actual = readSync(disk.fd, bytes, 0, length, offset);
    return bytes.buffer.slice(0, actual);
  }
}
globalThis.FileReaderSync = DiskFileReaderSync;

class DiskBlob extends Blob {
  constructor(disk) {
    super();
    this.disk = disk;
    this.snapshotSize = fstatSync(disk.fd).size;
  }
  get size() {
    return this.snapshotSize;
  }
  slice(start, end) {
    return { disk: this.disk, offset: start, length: end - start };
  }
  arrayBuffer() {
    throw new Error("whole-file reads are forbidden");
  }
}

function diskFile(t, bytes, size = bytes.byteLength) {
  const directory = mkdtempSync(resolve(tmpdir(), "polkavm-stream-"));
  const path = resolve(directory, "selected.bin");
  const fd = openSync(path, "w+");
  writeSync(fd, bytes);
  ftruncateSync(fd, size);
  t.after(() => {
    closeSync(fd);
    rmSync(directory, { recursive: true });
  });
  return { fd, path, reads: [] };
}

function diskSource(disk) {
  let fd = openSync(disk.path, "r");
  const state = { reads: [], closes: 0 };
  const source = {
    read(offset, length) {
      const bytes = new Uint8Array(length);
      state.reads.push([offset, length]);
      const actual = readSync(fd, bytes, 0, length, offset);
      return bytes.subarray(0, actual);
    },
    close() {
      state.closes++;
      if (fd !== null) {
        closeSync(fd);
        fd = null;
      }
    },
  };
  return { source, state };
}

function diskCache(t) {
  const directory = mkdtempSync(resolve(tmpdir(), "polkavm-cache-"));
  const path = resolve(directory, "scratch.bin");
  let fd = openSync(path, "wx+", 0o600);
  const state = { resets: [], writes: [], reads: [], flushes: 0, closes: 0 };
  const cache = {
    size: () => fstatSync(fd).size,
    reset(size) {
      state.resets.push(size);
      ftruncateSync(fd, 0);
      ftruncateSync(fd, size);
    },
    write(offset, bytes) {
      state.writes.push([offset, bytes.byteLength]);
      return writeSync(fd, bytes, 0, bytes.byteLength, offset);
    },
    read(offset, length) {
      state.reads.push([offset, length]);
      const bytes = new Uint8Array(length);
      return bytes.subarray(0, readSync(fd, bytes, 0, length, offset));
    },
    flush() {
      state.flushes++;
      fsyncSync(fd);
    },
    close() {
      state.closes++;
      if (fd !== null) {
        closeSync(fd);
        fd = null;
        rmSync(path);
      }
    },
  };
  t.after(() => {
    if (fd !== null) closeSync(fd);
    rmSync(directory, { recursive: true, force: true });
  });
  return { cache, state, path, get fd() { return fd; } };
}

function deferred() {
  let resolvePromise;
  const promise = new Promise((resolve) => { resolvePromise = resolve; });
  return { promise, resolve: resolvePromise };
}

function bytesBuffer(bytes) {
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  );
}

function descriptors(records) {
  const chunks = [];
  for (const record of records) {
    const payload =
      typeof record === "string" ? encoder.encode(record) : new Uint8Array();
    const header = new DataView(new ArrayBuffer(8));
    if (typeof record === "string") {
      header.setUint32(0, 1, true);
      header.setUint32(4, payload.byteLength, true);
    } else if (record.camera) {
      header.setUint32(0, 2, true);
    } else if (record.cancel !== undefined) {
      header.setUint32(0, 4, true);
      header.setUint32(4, record.cancel, true);
    } else {
      header.setUint32(0, 3, true);
      header.setUint32(4, record.trigger, true);
    }
    chunks.push(new Uint8Array(header.buffer), payload);
  }
  const bytes = new Uint8Array(
    chunks.reduce((total, chunk) => total + chunk.byteLength, 0) + 8,
  );
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

function nextMessage(messages, from, type) {
  return new Promise((resolvePromise, reject) => {
    const deadline = Date.now() + 10_000;
    const poll = () => {
      const message = messages
        .slice(from)
        .find((candidate) => candidate.type === type);
      if (message) {
        resolvePromise(message);
      } else if (Date.now() > deadline) {
        reject(new Error(`timed out waiting for ${type}`));
      } else {
        setTimeout(poll, 5);
      }
    };
    poll();
  });
}

async function launch({
  records = [],
  assets = [],
  fileInput = BOTH,
  fileRelaunch,
  fileCache,
  createFileCache,
  forceInterpreter,
  expectFailure = false,
  guest = program,
  cacheKey = "file-input",
}) {
  const messages = [];
  const receiver = {
    onmessage: null,
    postMessage(message) {
      messages.push(message);
      if (message.type === "compiled") {
        compiledPrograms.set(message.cacheKey, message.program);
      }
    },
  };
  globalThis.createPolkaVmRuntime(receiver, { createFileCache });
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(guest),
      compiledProgram: compiledPrograms.get(cacheKey),
      assets: [
        { path: "descriptors", bytes: bytesBuffer(descriptors(records)) },
        ...assets,
      ],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey,
      mediatedInputKinds: ["camera-ur"],
      fileInput: fileInput === "absent" ? undefined : fileInput,
      fileRelaunch,
      fileCache,
      forceInterpreter,
    },
  });
  if (expectFailure) {
    await nextMessage(messages, 0, "terminated");
    return { messages, receiver };
  }
  const save = await nextMessage(messages, 0, "save");
  const view = new DataView(save.bytes.buffer, save.bytes.byteOffset);
  const results = Array.from(
    { length: Math.min(16, save.bytes.byteLength / 4) },
    (_, index) => view.getInt32(index * 4, true),
  );
  return { messages, receiver, results };
}

function snapshot(saved) {
  assert.equal(saved.byteLength, HEADER_BYTES + CANCEL_OFFSET + 4);
  const bytes = saved.subarray(HEADER_BYTES);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const probes = Array.from({ length: 4 }, (_, index) => {
    const base = index * PROBE_STRIDE;
    const info = view.getInt32(base + 4, true);
    const read = view.getInt32(base + 8, true);
    return {
      status: view.getUint32(base, true),
      info:
        info > 0
          ? decoder.decode(bytes.subarray(base + 12, base + 12 + info))
          : info,
      read:
        read > 0
          ? decoder.decode(
              bytes.subarray(base + 12 + INFO_BYTES, base + 12 + INFO_BYTES + read),
            )
          : read,
    };
  });
  const assetLength = view.getInt32(ASSET_OFFSET, true);
  return {
    probes,
    asset: decoder.decode(
      bytes.subarray(ASSET_OFFSET + 4, ASSET_OFFSET + 4 + assetLength),
    ),
    cancel: view.getUint32(CANCEL_OFFSET, true),
  };
}

async function snapshotAfter(messages, from) {
  while (true) {
    const save = await nextMessage(messages, from, "save");
    if (save.bytes.byteLength !== 64) {
      return snapshot(save.bytes);
    }
    from = messages.indexOf(save) + 1;
  }
}

async function deliver(receiver, messages, handle, name, mimeType, bytes) {
  const from = messages.length;
  receiver.onmessage({
    data: {
      type: "file-input",
      handle,
      name,
      mimeType,
      bytes: bytesBuffer(encoder.encode(bytes)),
    },
  });
  return { delivery: await nextMessage(messages, from, "file-input-delivery"), from };
}

async function stop(receiver, messages) {
  const from = messages.length;
  receiver.onmessage({ data: { type: "stop" } });
  await nextMessage(messages, from, "terminated");
}

test("both browser backends apply the shared descriptor vectors", async () => {
  for (const forceInterpreter of BACKENDS) {
    for (const vector of vectors) {
      const { receiver, messages, results } = await launch({
        records: [vector.descriptor],
        forceInterpreter,
      });
      assert.equal(
        results[0],
        vector.valid ? 1 : -1,
        `${forceInterpreter}: ${vector.descriptor}`,
      );
      await stop(receiver, messages);
    }
  }
});

test("both browser backends share handles, quota, and delivery support", async () => {
  const other = (id) =>
    `{"id":"${id}","label":"L","extensions":[".${id}"],"delivery":"inline","maxBytes":1}`;
  const reordered =
    ' { "maxBytes" : 16, "delivery" : "inline", "mimeTypes" : ["text/plain"], "extensions" : [".txt"], "label" : "Text document", "id" : "doc" } ';
  for (const forceInterpreter of BACKENDS) {
    const { receiver, messages, results } = await launch({
      records: [
        INLINE,
        { camera: true },
        reordered,
        INLINE.replace('"maxBytes":16', '"maxBytes":17'),
        RELAUNCH,
        RELAUNCH.replace('"id":"rom"', '"id":"rom-2"'),
        RELAUNCH.replace('"id":"rom"', '"id":"rom-3"').replace(MOUNT_PATH, ENTRYPOINT),
        ...["a", "b", "c", "d", "e", "f"].map(other),
        { camera: true },
      ],
      forceInterpreter,
    });
    assert.deepEqual(results.slice(0, 14), [1, 2, 1, -1, 3, -1, -1, 4, 5, 6, 7, 8, -3, 2]);
    const registrations = messages.filter(
      (message) => message.type === "file-registrations",
    );
    const latest = registrations.at(-1).registrations;
    assert.deepEqual(
      latest.map((registration) => registration.handle),
      [1, 3, 4, 5, 6, 7, 8],
    );
    assert.deepEqual(latest[0].descriptor, JSON.parse(INLINE));
    assert.deepEqual(latest[1].descriptor, {
      ...JSON.parse(RELAUNCH),
      mimeTypes: [],
    });
    await stop(receiver, messages);

    for (const [fileInput, expected] of [
      ["absent", [-2, -2, -2]],
      [{ inline: false, relaunch: false, entrypoint: "" }, [-2, -2, -2]],
      [{ inline: true, relaunch: false, entrypoint: ENTRYPOINT }, [1, -4, -4]],
      [{ inline: false, relaunch: true, entrypoint: ENTRYPOINT }, [-4, 1, -4]],
      [{ inline: false, relaunch: false, stream: true, entrypoint: ENTRYPOINT }, [-4, -4, 1]],
    ]) {
      const launched = await launch({
        records: [INLINE, RELAUNCH, STREAM],
        fileInput,
        forceInterpreter,
      });
      assert.deepEqual(launched.results.slice(0, 3), expected);
      await stop(launched.receiver, launched.messages);
    }
  }
});

test("both browser backends deliver a triggered inline file with its info", async () => {
  for (const forceInterpreter of BACKENDS) {
    const { receiver, messages, results } = await launch({
      records: [INLINE, { trigger: 0 }],
      forceInterpreter,
    });
    assert.deepEqual(results.slice(0, 2), [1, 0]);
    const request = await nextMessage(messages, 0, "file-input-request");
    assert.equal(request.handle, 1);
    assert.deepEqual(request.descriptor, JSON.parse(INLINE));

    const { delivery, from } = await deliver(
      receiver,
      messages,
      1,
      "notes/Draft\u0001.txt",
      "text/plain",
      "hello",
    );
    assert.equal(delivery.outcome, "ready");
    const ready = await snapshotAfter(messages, from);
    assert.equal(ready.probes[0].status, 3);
    assert.equal(
      ready.probes[0].info,
      '{"name":"Draft�.txt","mimeType":"text/plain","size":5}',
    );
    assert.equal(ready.probes[0].read, "hello");
    assert.equal(ready.probes[1].info, -1);
    const consumed = await snapshotAfter(messages, messages.length);
    assert.deepEqual(consumed.probes[0], { status: 1, info: 0, read: 0 });
    await stop(receiver, messages);
  }
});

test("both browser backends activate idle registrations and reject over-bound files", async () => {
  for (const forceInterpreter of BACKENDS) {
    const { receiver, messages } = await launch({
      records: [INLINE, { camera: true }, { trigger: 1 }],
      forceInterpreter,
    });
    await nextMessage(messages, 0, "mediated-input-request");
    let { delivery } = await deliver(receiver, messages, 1, "a.txt", "", "a");
    assert.equal(delivery.outcome, "refused");
    receiver.onmessage({
      data: {
        type: "mediated-input-result",
        handle: 2,
        status: 4,
        bytes: new ArrayBuffer(0),
      },
    });

    let from;
    ({ delivery, from } = await deliver(
      receiver,
      messages,
      1,
      "big.txt",
      "",
      "x".repeat(17),
    ));
    assert.equal(delivery.outcome, "rejected");
    assert.deepEqual((await snapshotAfter(messages, from)).probes[0], {
      status: 6,
      info: 0,
      read: 0,
    });
    ({ delivery, from } = await deliver(receiver, messages, 1, "empty.txt", "", ""));
    assert.equal(delivery.outcome, "rejected");

    ({ delivery, from } = await deliver(
      receiver,
      messages,
      1,
      "dropped.txt",
      "",
      "drop",
    ));
    assert.equal(delivery.outcome, "ready");
    ({ delivery } = await deliver(receiver, messages, 1, "again.txt", "", "x"));
    assert.equal(delivery.outcome, "refused");
    assert.equal((await snapshotAfter(messages, from)).probes[0].read, "drop");

    const failed = messages.length;
    receiver.onmessage({
      data: {
        type: "file-input",
        handle: 1,
        name: "a.txt",
        mimeType: "Text/Plain",
        bytes: new ArrayBuffer(1),
      },
    });
    await nextMessage(messages, failed, "terminated");
    assert.ok(messages.slice(failed).some((message) => message.type === "error"));
  }
});

test("both browser backends relaunch with the file mounted over the asset", async () => {
  const packaged = {
    path: MOUNT_PATH,
    bytes: bytesBuffer(encoder.encode("packaged cartridge")),
  };
  for (const forceInterpreter of BACKENDS) {
    const first = await launch({
      records: [INLINE, RELAUNCH],
      assets: [packaged],
      forceInterpreter,
    });
    assert.equal(
      (await snapshotAfter(first.messages, 0)).asset,
      "packaged cartridge",
    );
    const { delivery, from } = await deliver(
      first.receiver,
      first.messages,
      2,
      "roms/Game.bin",
      "application/octet-stream",
      "user cartridge",
    );
    assert.equal(delivery.outcome, "relaunch");
    await nextMessage(first.messages, from, "terminated");
    const { bytes, ...metadata } = delivery.relaunch;
    assert.deepEqual(metadata, {
      id: "rom",
      mountPath: MOUNT_PATH,
      name: "Game.bin",
      mimeType: "application/octet-stream",
    });
    assert.equal(decoder.decode(bytes), "user cartridge");

    const second = await launch({
      records: [INLINE, RELAUNCH],
      assets: [packaged],
      fileRelaunch: delivery.relaunch,
      forceInterpreter,
    });
    assert.deepEqual(second.results.slice(0, 2), [1, 2]);
    const mounted = await snapshotAfter(second.messages, 0);
    assert.equal(mounted.asset, "user cartridge");
    assert.deepEqual(mounted.probes[1], {
      status: 3,
      info: '{"name":"Game.bin","mimeType":"application/octet-stream","size":14}',
      read: 0,
    });
    const acknowledged = await snapshotAfter(
      second.messages,
      second.messages.length,
    );
    assert.deepEqual(acknowledged.probes[1], { status: 1, info: 0, read: 0 });
    assert.equal(acknowledged.asset, "user cartridge");
    await stop(second.receiver, second.messages);
  }
});

test("both browser backends cancel a ready file back to idle", async () => {
  for (const forceInterpreter of BACKENDS) {
    const inline = await launch({
      records: [INLINE, { cancel: 0 }],
      forceInterpreter,
    });
    assert.equal((await snapshotAfter(inline.messages, 0)).cancel, 2);
    const { delivery, from } = await deliver(
      inline.receiver,
      inline.messages,
      1,
      "a.txt",
      "",
      "abc",
    );
    assert.equal(delivery.outcome, "ready");
    const cancelled = await snapshotAfter(inline.messages, from);
    assert.equal(cancelled.cancel, 0);
    assert.deepEqual(cancelled.probes[0], { status: 1, info: 0, read: 0 });
    assert.equal(
      inline.messages
        .slice(from)
        .some((message) => message.type === "mediated-input-cancel"),
      false,
    );
    await stop(inline.receiver, inline.messages);

    const active = await launch({
      records: [INLINE, { trigger: 0 }, { cancel: 0 }],
      forceInterpreter,
    });
    const request = await nextMessage(active.messages, 0, "file-input-request");
    const stopped = await snapshotAfter(active.messages, 0);
    assert.equal(stopped.cancel, 0);
    assert.equal(stopped.probes[0].status, 1);
    assert.equal(
      (await nextMessage(active.messages, 0, "mediated-input-cancel")).handle,
      request.handle,
    );
    await stop(active.receiver, active.messages);

    const relaunched = await launch({
      records: [RELAUNCH, { cancel: 0 }],
      fileRelaunch: {
        id: "rom",
        mountPath: MOUNT_PATH,
        name: "Game.bin",
        mimeType: "",
        bytes: bytesBuffer(encoder.encode("user cartridge")),
      },
      forceInterpreter,
    });
    const released = await snapshotAfter(relaunched.messages, 0);
    assert.equal(released.cancel, 0);
    assert.deepEqual(released.probes[0], { status: 1, info: 0, read: 0 });
    assert.equal(released.asset, "user cartridge");
    await stop(relaunched.receiver, relaunched.messages);
  }
});

test("both browser backends report registrations of an execution that fails in init", async () => {
  for (const forceInterpreter of BACKENDS) {
    const { messages } = await launch({
      records: [INLINE, RELAUNCH, { trigger: 1 << 20 }],
      forceInterpreter,
      expectFailure: true,
    });
    const registrations = messages.filter(
      (message) => message.type === "file-registrations",
    );
    assert.ok(registrations.length > 0);
    assert.ok(
      messages.indexOf(registrations.at(-1)) <
        messages.findIndex((message) => message.type === "terminated"),
    );
    assert.deepEqual(
      registrations.at(-1).registrations.map(({ descriptor }) => descriptor.id),
      ["doc", "rom"],
    );
    assert.ok(messages.some((message) => message.type === "error"));
  }
});

test("both browser backends cancel an active file request on teardown", async () => {
  for (const forceInterpreter of BACKENDS) {
    const { receiver, messages } = await launch({
      records: [INLINE, { trigger: 0 }],
      forceInterpreter,
    });
    const request = await nextMessage(messages, 0, "file-input-request");
    await stop(receiver, messages);
    const cancel = messages.find(
      (message) => message.type === "mediated-input-cancel",
    );
    assert.equal(cancel?.handle, request.handle);
  }
});

test("stream selection keeps metadata without reading or acknowledging the Blob", async () => {
  class UnreadBlob extends Blob {
    arrayBuffer() {
      throw new Error("selection must not read a whole Blob");
    }
    slice() {
      throw new Error("selection must not read any Blob range");
    }
  }
  for (const forceInterpreter of BACKENDS) {
    const { receiver, messages } = await launch({
      records: [STREAM],
      forceInterpreter,
    });
    try {
      let from = messages.length;
      receiver.onmessage({
        data: {
          type: "file-input",
          handle: 1,
          name: "/private/game/data.bin",
          mimeType: "",
          file: new UnreadBlob(["stream contents"]),
        },
      });
      assert.equal(
        (await nextMessage(messages, from, "file-input-delivery")).outcome,
        "ready",
      );
      const expected = {
        status: 3,
        info: '{"name":"data.bin","mimeType":"","size":15}',
        read: 0,
      };
      assert.deepEqual((await snapshotAfter(messages, from)).probes[0], expected);
      assert.deepEqual(
        (await snapshotAfter(messages, messages.length)).probes[0],
        expected,
      );
      from = messages.length;
      receiver.onmessage({
        data: {
          type: "file-input",
          handle: 1,
          name: "replacement.bin",
          mimeType: "",
          file: new UnreadBlob(["different"]),
        },
      });
      assert.equal(
        (await nextMessage(messages, from, "file-input-delivery")).outcome,
        "refused",
      );
      assert.deepEqual((await snapshotAfter(messages, from)).probes[0], expected);
      await stop(receiver, messages);
      assert.equal(
        messages.filter((message) => message.type === "mediated-input-cancel").length,
        1,
      );
    } finally {
      if (receiver.onmessage) {
        await stop(receiver, messages);
      }
    }
  }
});

test("relaunch files count toward the browser asset bounds", async () => {
  const assets = [
    { path: "a.bin", bytes: new ArrayBuffer(128 * 1024 * 1024) },
    // With the eight-byte descriptors asset the launch holds 256 MiB - 8.
    { path: "b.bin", bytes: new ArrayBuffer(128 * 1024 * 1024 - 16) },
  ];
  const relaunch = (mountPath, length) => ({
    id: "rom",
    mountPath,
    name: "Game.bin",
    mimeType: "",
    bytes: new ArrayBuffer(length),
  });
  const { messages } = await launch({
    records: [],
    assets,
    fileRelaunch: relaunch(MOUNT_PATH, 9),
    forceInterpreter: true,
    expectFailure: true,
  });
  assert.match(
    messages.find((message) => message.type === "error").message,
    /relaunch file exceeds the asset bounds/,
  );
});

function streamCommands(records) {
  const bytes = new Uint8Array(records.length * 20);
  const view = new DataView(bytes.buffer);
  records.forEach((record, index) => {
    for (let field = 0; field < 5; field++) {
      view.setUint32(index * 20 + field * 4, record[field] ?? 0, true);
    }
  });
  return bytes;
}

function streamSnapshot(bytes) {
  assert.equal(bytes.byteLength, 65812);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const result = view.getInt32(0, true);
  const infoLength = view.getInt32(8, true);
  return {
    result,
    status: view.getUint32(4, true),
    info: infoLength > 0
      ? JSON.parse(decoder.decode(bytes.subarray(20, 20 + infoLength)))
      : infoLength,
    inputRead: view.getInt32(12, true),
    statusAfter: view.getUint32(16, true),
    bytes: bytes.slice(276, 276 + Math.max(result, 0)),
  };
}

async function streamSnapshotAfter(messages, from) {
  while (true) {
    const save = await nextMessage(messages, from, "save");
    if (save.bytes.byteLength === 65812) {
      return streamSnapshot(save.bytes);
    }
    from = messages.indexOf(save) + 1;
  }
}

async function launchStream(t, records, forceInterpreter, cacheOptions = {}) {
  const launched = await launch({
    guest: streamProgram,
    cacheKey: "file-stream",
    assets: [{
      path: "stream-commands",
      bytes: bytesBuffer(streamCommands([[0, 1], ...records])),
    }],
    fileInput: { inline: false, relaunch: false, stream: true, entrypoint: ENTRYPOINT },
    forceInterpreter,
    ...cacheOptions,
  });
  const { receiver, messages } = launched;
  t.after(async () => {
    if (receiver.onmessage) {
      await stop(receiver, messages);
    }
  });
  assert.equal(
    (await nextMessage(messages, 0, "ready")).backend,
    forceInterpreter ? "interpreter" : "compiler",
  );
  assert.equal((await streamSnapshotAfter(messages, 0)).status, 1);
  return {
    ...launched,
    async select(file, expected = "ready", handle = 1) {
      const from = messages.length;
      receiver.onmessage({
        data: { type: "file-input", handle, name: "selected.bin", mimeType: "", file },
      });
      assert.equal(
        (await nextMessage(messages, from, "file-input-delivery")).outcome,
        expected,
      );
      return streamSnapshotAfter(messages, from);
    },
    async step() {
      const from = messages.length;
      receiver.onmessage({
        data: { type: "input", bytes: new Uint8Array([1, 4, 0, 0, 0, 0, 0, 0]).buffer },
      });
      return streamSnapshotAfter(messages, from);
    },
  };
}

test("both browser cores perform bounded synchronous disk ranges and preserve invalid requests", async (t) => {
  const size = 128 * 1024 * 1024 + 33;
  const disk = diskFile(t, encoder.encode("abcdefghijkl"), size);
  writeSync(disk.fd, encoder.encode("YZ"), 0, 2, size - 2);
  const replacement = diskFile(t, encoder.encode("not the selected file"));
  for (const forceInterpreter of BACKENDS) {
    disk.reads.length = 0;
    const live = await launchStream(t, [
      [1, 1, 2, 4],
      [1, 1, size - 2, 8],
      [1, 1, size, 1],
      [1, 1, size + 1, 1],
      [1, 1, 0, 0],
      [1, 1, 0, 65537],
      [4, 1, 0, 4],
      [1, 2, 0, 4],
      [1, 1, 0, 65536],
      [2, 1],
      [1, 1, 0, 4],
    ], forceInterpreter);
    let saved = await live.select(new DiskBlob(disk));
    assert.equal(saved.result, 4);
    assert.equal(decoder.decode(saved.bytes), "cdef", `interpreter=${forceInterpreter}`);
    assert.deepEqual(saved.info, { name: "selected.bin", mimeType: "", size });
    assert.equal(saved.inputRead, 0);
    assert.equal(saved.statusAfter, 3);
    // The Wasm i32 idle sentinel must not become a negative timer delay and
    // consume subsequent commands while the guest has requested sleep.
    await new Promise((resolve) => setTimeout(resolve, 40));
    assert.deepEqual(disk.reads, [[2, 4]]);
    saved = await live.select(new DiskBlob(replacement), "refused");
    assert.equal(saved.result, 2);
    assert.equal(decoder.decode(saved.bytes), "YZ");
    for (const expected of [0, -2, -2, -2, -3]) {
      saved = await live.step();
      assert.equal(saved.result, expected);
      assert.equal(saved.status, 3);
      assert.equal(saved.statusAfter, 3);
      assert.equal(saved.info.size, size);
    }
    assert.equal((await live.step()).result, -1);
    assert.deepEqual(disk.reads, [[2, 4], [size - 2, 2]]);
    assert.deepEqual(replacement.reads, []);
    saved = await live.step();
    assert.equal(saved.result, 65536);
    assert.equal(saved.statusAfter, 3);
    assert.equal(saved.info.size, size);
    assert.deepEqual(saved.bytes.subarray(0, 12), encoder.encode("abcdefghijkl"));
    assert.ok(saved.bytes.subarray(12).every((byte) => byte === 0));
    assert.deepEqual(disk.reads.at(-1), [0, 65536]);
    saved = await live.step();
    assert.equal(saved.result, 0);
    assert.equal(saved.status, 1);
    assert.equal(saved.info, 0);
    assert.equal((await live.step()).result, -1);
    assert.equal(
      live.messages.filter((message) => message.type === "mediated-input-cancel").length,
      1,
    );
    assert.equal(
      live.messages.some((message) => message.type === "file-input-request"),
      false,
    );
    await stop(live.receiver, live.messages);
  }
});

test("both browser cores close a truncated stream on short disk read", async (t) => {
  for (const forceInterpreter of BACKENDS) {
    const disk = diskFile(t, encoder.encode("abcd"));
    const live = await launchStream(t, [[0, 1], [1, 1, 0, 4], [1, 1, 0, 1]], forceInterpreter);
    assert.equal((await live.select(new DiskBlob(disk))).status, 3);
    ftruncateSync(disk.fd, 1);
    const failed = await live.step();
    assert.equal(failed.result, -4, `interpreter=${forceInterpreter}`);
    assert.equal(failed.status, 6);
    assert.equal(failed.info, 0);
    assert.equal((await live.step()).result, -1);
    assert.deepEqual(disk.reads, [[0, 4]]);
    assert.equal(
      live.messages.filter((message) => message.type === "mediated-input-cancel").length,
      1,
    );
    await stop(live.receiver, live.messages);
  }
});

async function translatedStream(t, records) {
  if (!compiledPrograms.has("file-stream")) {
    const compiled = await launchStream(t, [], false);
    await stop(compiled.receiver, compiled.messages);
  }
  const messages = [];
  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiledPrograms.get("file-stream"),
    [{ path: "stream-commands", bytes: streamCommands(records) }],
    (message) => messages.push(message),
    1_000_000, false, "framebuffer", null, 0, [],
    { inline: false, relaunch: false, stream: true, entrypoint: ENTRYPOINT },
  );
  t.after(() => translated.stop());
  translated.initialize();
  assert.equal(new DataView(messages.find((message) => message.type === "save").bytes.buffer).getUint32(0, true), 1);
  return {
    translated, messages,
    step() {
      translated.update(0);
      return streamSnapshot(messages.findLast((message) => message.type === "save").bytes);
    },
  };
}

test("translated stream ownership closes only refused candidates and releases on trigger/cancel/stop", async (t) => {
  const disk = diskFile(t, encoder.encode("abcdef"));
  const live = await translatedStream(t, [[1, 1, 1, 3], [3, 1], [1, 1, 2, 4], [2, 1]]);
  const first = diskSource(disk);
  const refused = diskSource(disk);
  const selected = (source, size = 6) =>
    live.translated.sendFileStream(1, "/private/selected.bin", "", size, source, null);
  assert.deepEqual(selected(first.source), { outcome: "ready" });
  assert.deepEqual(selected(refused.source), { outcome: "refused" });
  assert.equal(refused.state.closes, 1);
  assert.equal(first.state.closes, 0);
  assert.throws(
    () => live.translated.deliverFile(1, "bytes.bin", "", encoder.encode("wrong delivery")),
    /metadata-only delivery/,
  );
  assert.equal(decoder.decode(live.step().bytes), "bcd");
  assert.equal(live.step().status, 2);
  assert.equal(first.state.closes, 1);
  assert.deepEqual(
    live.messages.filter((message) => ["mediated-input-cancel", "file-input-request"].includes(message.type))
      .map((message) => message.type),
    ["mediated-input-cancel", "file-input-request"],
  );
  const second = diskSource(disk);
  assert.deepEqual(selected(second.source), { outcome: "ready" });
  assert.equal(decoder.decode(live.step().bytes), "cdef");
  assert.equal(live.step().status, 1);
  assert.equal(second.state.closes, 1);
  for (const size of [0, 0x100000000]) {
    const rejected = diskSource(disk);
    assert.deepEqual(selected(rejected.source, size), { outcome: "rejected" });
    assert.equal(rejected.state.closes, 1);
    assert.deepEqual(rejected.state.reads, []);
  }
  const final = diskSource(disk);
  assert.deepEqual(selected(final.source), { outcome: "ready" });
  live.translated.stop();
  live.translated.stop();
  assert.equal(final.state.closes, 1);
  assert.equal(first.state.closes, 1);
  assert.equal(second.state.closes, 1);
});

test("translated synchronous reads validate guest memory before disk and close short sources", async (t) => {
  const disk = diskFile(t, encoder.encode("abcdef"));
  const live = await translatedStream(t, [[4, 1, 0, 3], [1, 1, 0, 3], [1, 1, 0, 6], [1, 1, 0, 1]]);
  const { source, state } = diskSource(disk);
  live.translated.sendFileStream(1, "selected.bin", "", 6, source, null);
  assert.equal(live.step().result, -3);
  assert.deepEqual(state.reads, []);
  assert.equal(decoder.decode(live.step().bytes), "abc");
  ftruncateSync(disk.fd, 1);
  const failed = live.step();
  assert.equal(failed.result, -4);
  assert.equal(failed.status, 6);
  assert.equal(failed.info, 0);
  assert.equal(state.closes, 1);
  assert.equal(live.step().result, -1);
  live.translated.stop();
  assert.equal(state.closes, 1);
});

test("core refuses stream support without a synchronous worker reader", async () => {
  const reader = globalThis.FileReaderSync;
  delete globalThis.FileReaderSync;
  try {
    const { messages } = await launch({ records: [STREAM], expectFailure: true });
    assert.match(
      messages.find((message) => message.type === "error").message,
      /require a worker with FileReaderSync/,
    );
    const inline = await launch({
      records: [INLINE],
      fileInput: { inline: true, relaunch: false, entrypoint: ENTRYPOINT },
    });
    assert.equal(inline.results[0], 1);
    await stop(inline.receiver, inline.messages);
  } finally {
    globalThis.FileReaderSync = reader;
  }
});

test("both browser cores replace a stream only after closing it for a new picker", async (t) => {
  const first = diskFile(t, encoder.encode("first"));
  const second = diskFile(t, encoder.encode("second"));
  for (const forceInterpreter of BACKENDS) {
    const live = await launchStream(t, [[0, 1], [3, 1], [1, 1, 1, 3], [2, 1], [1, 1, 0, 1]], forceInterpreter);
    assert.equal((await live.select(new DiskBlob(first))).status, 3);
    const picking = await live.step();
    assert.equal(picking.status, 2, `interpreter=${forceInterpreter}`);
    assert.equal(picking.info, 0);
    assert.deepEqual(
      live.messages.filter((message) => ["mediated-input-cancel", "file-input-request"].includes(message.type))
        .map((message) => message.type),
      ["mediated-input-cancel", "file-input-request"],
    );
    const read = await live.select(new DiskBlob(second));
    assert.equal(decoder.decode(read.bytes), "eco");
    assert.equal(read.info.size, 6);
    assert.equal((await live.step()).status, 1);
    assert.equal((await live.step()).result, -1);
    assert.deepEqual(first.reads, []);
    await stop(live.receiver, live.messages);
  }
});

test("both browser cores reject empty and oversized Blob metadata without reading", async (t) => {
  const empty = diskFile(t, new Uint8Array());
  const oversized = diskFile(t, encoder.encode("a"), 0x100000000);
  for (const forceInterpreter of BACKENDS) {
    const live = await launchStream(t, [[0, 1], [0, 1]], forceInterpreter);
    for (const disk of [empty, oversized]) {
      const rejected = await live.select(new DiskBlob(disk), "rejected");
      assert.equal(rejected.status, 6);
      assert.equal(rejected.info, 0);
      assert.deepEqual(disk.reads, []);
    }
    await stop(live.receiver, live.messages);
  }
});

test("both browser caches enforce sequential sealed disk I/O without changing the original", async (t) => {
  const original = diskFile(t, encoder.encode("original"));
  for (const forceInterpreter of BACKENDS) {
    const disk = diskCache(t);
    const live = await launchStream(t, [
      [8, 1, 0, 4], [6, 1, 0, 4], [7, 1], [5, 1, 65540],
      [5, 1, 0], [5, 1, 512 * 1024 * 1024 + 1],
      [6, 1, 1, 4], [6, 1, 0, 0], [6, 1, 0, 65537], [9, 1, 0, 4],
      [6, 1, 0, 65536], [7, 1], [8, 1, 0, 1], [6, 1, 65536, 4],
      [7, 1], [7, 1], [6, 1, 65540, 1], [10, 1, 0, 4],
      [8, 1, 0, 65536], [8, 1, 65538, 8], [8, 1, 65540, 1],
      [8, 1, 65541, 1], [8, 1, 0, 0], [8, 1, 0, 65537],
      [1, 1, 0, 8], [5, 1, 4], [8, 1, 0, 4], [6, 1, 0, 4],
      [7, 1], [8, 1, 0, 4], [2, 1], [8, 1, 0, 1],
    ], forceInterpreter, { fileCache: true, createFileCache: async () => disk.cache });
    assert.equal((await live.select(new DiskBlob(original))).result, -1);
    for (const result of [-2, -2, 0, -2, -2, -2, -2, -2, -3]) {
      assert.equal((await live.step()).result, result, `interpreter=${forceInterpreter}`);
    }
    assert.deepEqual(disk.state.resets, [65540]);
    assert.deepEqual(disk.state.writes, []);
    assert.deepEqual(disk.state.reads, []);
    for (const result of [65536, -2, -1, 4, 0, -2, -2, -3]) {
      assert.equal((await live.step()).result, result);
    }
    assert.deepEqual(disk.state.writes, [[0, 65536], [65536, 4]]);
    assert.deepEqual(disk.state.reads, []);
    const prefix = await live.step();
    assert.equal(prefix.result, 65536);
    assert.ok(prefix.bytes.every((byte, index) => byte === (index & 255)));
    assert.deepEqual((await live.step()).bytes, new Uint8Array([2, 3]));
    for (const result of [0, -2, -2, -2]) {
      assert.equal((await live.step()).result, result);
    }
    assert.deepEqual(disk.state.reads, [[0, 65536], [65538, 2]]);
    const source = await live.step();
    assert.equal(decoder.decode(source.bytes), "original");
    assert.deepEqual(source.info, { name: "selected.bin", mimeType: "", size: 8 });
    for (const result of [0, -1, 4, 0]) {
      assert.equal((await live.step()).result, result);
    }
    assert.deepEqual((await live.step()).bytes, new Uint8Array([0, 1, 2, 3]));
    assert.equal((await live.step()).status, 1);
    assert.equal(disk.state.closes, 1);
    assert.equal(existsSync(disk.path), false);
    assert.equal((await live.step()).result, -1);
    assert.equal(decoder.decode(await readFile(original.path)), "original");
    await stop(live.receiver, live.messages);
  }
});

test("both browser caches reserve the aggregate disk budget before resize and release it on cancel", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  const maximum = 512 * 1024 * 1024;
  for (const forceInterpreter of BACKENDS) {
    const disks = [diskCache(t), diskCache(t)];
    let next = 0;
    const live = await launchStream(t, [
      [11, 1, 0], [5, 1, maximum - 4], [5, 2, 5], [5, 2, 4],
      [5, 1, maximum], [5, 1, 4], [5, 2, maximum - 4],
      [2, 1], [5, 2, maximum],
    ], forceInterpreter, { fileCache: true, createFileCache: async () => disks[next++].cache });
    assert.equal((await live.select(new DiskBlob(original))).result, 2);
    assert.equal((await live.select(new DiskBlob(original), "ready", 2)).result, 0);
    assert.equal((await live.step()).result, -2);
    assert.deepEqual(disks[1].state.resets, []);
    assert.equal((await live.step()).result, 0);
    assert.equal((await live.step()).result, -2);
    assert.deepEqual(disks[0].state.resets, [maximum - 4]);
    for (const result of [0, 0, 0, 0]) assert.equal((await live.step()).result, result);
    assert.deepEqual(disks[1].state.resets, [4, maximum - 4, maximum]);
    assert.equal(disks[0].state.closes, 1);
    await stop(live.receiver, live.messages);
    assert.equal(disks[1].state.closes, 1);
  }
});

test("cache disk failures drop only derived bytes and preserve the Ready original in both cores", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  for (const forceInterpreter of BACKENDS) {
    for (const failure of ["read", "write", "reset", "flush"]) {
      const disk = diskCache(t);
      let commands;
      if (failure === "write") {
        const write = disk.cache.write;
        disk.cache.write = (offset, bytes) => write(offset, bytes.subarray(0, bytes.byteLength - 1));
        commands = [[5, 1, 4], [6, 1, 0, 4]];
      } else if (failure === "reset") {
        const reset = disk.cache.reset;
        disk.cache.reset = (size) => { reset(size); ftruncateSync(disk.fd, size - 1); };
        commands = [[0, 1], [5, 1, 4]];
      } else if (failure === "flush") {
        disk.cache.flush = () => fsyncSync(-1);
        commands = [[5, 1, 4], [6, 1, 0, 4], [7, 1]];
      } else {
        commands = [[5, 1, 4], [6, 1, 0, 4], [7, 1], [8, 1, 0, 4]];
      }
      const live = await launchStream(t, [
        ...commands, [8, 1, 0, 1], [1, 1, 0, 6],
      ], forceInterpreter, { fileCache: true, createFileCache: async () => disk.cache });
      await live.select(new DiskBlob(original));
      for (let index = 1; index < commands.length - 1; index++) await live.step();
      if (failure === "read") ftruncateSync(disk.fd, 1);
      const failed = await live.step();
      assert.equal(failed.result, -4, `${failure}, interpreter=${forceInterpreter}`);
      assert.equal(failed.status, 3);
      assert.equal(failed.info.size, 6);
      assert.equal(disk.state.closes, 1);
      assert.equal(existsSync(disk.path), false);
      assert.equal((await live.step()).result, -1);
      assert.equal(decoder.decode((await live.step()).bytes), "source");
      assert.equal(live.messages.some((message) => message.type === "mediated-input-cancel"), false);
      await stop(live.receiver, live.messages);
    }
  }
});

test("refused cached candidates and cache creation failure never replace a Ready source", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  const other = diskFile(t, encoder.encode("other"));
  for (const forceInterpreter of BACKENDS) {
    const accepted = diskCache(t);
    const refused = diskCache(t);
    let count = 0;
    const live = await launchStream(t, [
      [5, 1, 4], [6, 1, 0, 4], [7, 1], [8, 1, 0, 4], [1, 1, 0, 6],
    ], forceInterpreter, {
      fileCache: true,
      async createFileCache() {
        if (++count === 3) throw new Error("disk unavailable");
        return count === 1 ? accepted.cache : refused.cache;
      },
    });
    assert.equal((await live.select(new DiskBlob(original))).result, 0);
    assert.equal((await live.select(new DiskBlob(other), "refused")).result, 4);
    assert.equal(refused.state.closes, 1);
    assert.equal(accepted.state.closes, 0);
    assert.equal((await live.step()).result, 0);
    const from = live.messages.length;
    live.receiver.onmessage({
      data: { type: "file-input", handle: 1, name: "other.bin", mimeType: "", file: new DiskBlob(other) },
    });
    assert.equal((await nextMessage(live.messages, from, "file-input-delivery")).outcome, "error");
    assert.match((await nextMessage(live.messages, from, "error")).message, /disk unavailable/);
    assert.deepEqual((await live.step()).bytes, new Uint8Array([0, 1, 2, 3]));
    assert.equal(decoder.decode((await live.step()).bytes), "source");
    assert.deepEqual(other.reads, []);
    await stop(live.receiver, live.messages);
  }
});

test("cancel invalidates an asynchronous cache open without delivering a stale File", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  for (const forceInterpreter of BACKENDS) {
    const disk = diskCache(t);
    const opening = deferred();
    const started = deferred();
    const closed = deferred();
    t.after(() => opening.resolve());
    const close = disk.cache.close;
    disk.cache.close = () => { close(); closed.resolve(); };
    const live = await launchStream(t, [[3, 1], [2, 1], [0, 1]], forceInterpreter, {
      fileCache: true,
      async createFileCache() { started.resolve(); await opening.promise; return disk.cache; },
    });
    assert.equal((await live.step()).status, 2);
    const from = live.messages.length;
    live.receiver.onmessage({
      data: { type: "file-input", handle: 1, name: "source.bin", mimeType: "", file: new DiskBlob(original) },
    });
    await started.promise;
    assert.equal((await live.step()).status, 1);
    opening.resolve();
    await closed.promise;
    const saved = await live.step();
    assert.equal(saved.status, 1);
    assert.equal(saved.info, 0);
    assert.equal(live.messages.slice(from).some((message) => message.type === "file-input-delivery"), false);
    assert.equal(disk.state.closes, 1);
    assert.equal(existsSync(disk.path), false);
    await stop(live.receiver, live.messages);
  }
});

test("stop waits for pending and selected cache deletion before reporting termination", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  for (const forceInterpreter of BACKENDS) {
    for (const selected of [false, true]) {
      const disk = diskCache(t);
      const opening = deferred();
      const started = deferred();
      const closing = deferred();
      const deleteAllowed = deferred();
      t.after(() => { opening.resolve(); deleteAllowed.resolve(); });
      const close = disk.cache.close;
      disk.cache.close = async () => {
        closing.resolve();
        await deleteAllowed.promise;
        close();
      };
      const live = await launchStream(t, [], forceInterpreter, {
        fileCache: true,
        async createFileCache() { started.resolve(); await opening.promise; return disk.cache; },
      });
      live.receiver.onmessage({
        data: { type: "file-input", handle: 1, name: "source.bin", mimeType: "", file: new DiskBlob(original) },
      });
      await started.promise;
      if (selected) {
        opening.resolve();
        assert.equal((await nextMessage(live.messages, 0, "file-input-delivery")).outcome, "ready");
      }
      const from = live.messages.length;
      live.receiver.onmessage({ data: { type: "stop" } });
      await Promise.resolve();
      assert.equal(live.messages.slice(from).some((message) => message.type === "terminated"), false);
      opening.resolve();
      await closing.promise;
      assert.equal(existsSync(disk.path), true);
      assert.equal(live.messages.slice(from).some((message) => message.type === "terminated"), false);
      deleteAllowed.resolve();
      const terminated = await nextMessage(live.messages, from, "terminated");
      assert.equal(terminated.cleanupFailed, undefined);
      assert.equal(existsSync(disk.path), false);
      assert.equal(live.messages.slice(from).some((message) => message.type === "file-input-delivery"), false);
      assert.equal(disk.state.closes, 1);
    }
  }
});

test("normal cache stop reports deletion errors instead of claiming successful cleanup", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  for (const forceInterpreter of BACKENDS) {
    const disk = diskCache(t);
    const close = disk.cache.close;
    disk.cache.close = async () => close();
    const live = await launchStream(t, [[0, 1]], forceInterpreter, {
      fileCache: true, createFileCache: async () => disk.cache,
    });
    await live.select(new DiskBlob(original));
    renameSync(disk.path, `${disk.path}.retained`);
    const from = live.messages.length;
    live.receiver.onmessage({ data: { type: "stop" } });
    assert.equal((await nextMessage(live.messages, from, "terminated")).cleanupFailed, true);
    assert.match((await nextMessage(live.messages, from, "error")).message, /cleanup failed:.*ENOENT/);
    assert.equal(existsSync(`${disk.path}.retained`), true);
  }
});

test("private cache selection is opt-in and requires a real backend with stream support", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  for (const forceInterpreter of BACKENDS) {
    let opens = 0;
    const live = await launchStream(t, [[5, 1, 4], [1, 1, 0, 6]], forceInterpreter, {
      createFileCache: async () => { opens++; return diskCache(t).cache; },
    });
    assert.equal((await live.select(new DiskBlob(original))).result, -1);
    assert.equal(opens, 0);
    assert.equal(decoder.decode((await live.step()).bytes), "source");
    await stop(live.receiver, live.messages);
  }
  for (const [options, expected] of [
    [{ fileCache: true, fileInput: { inline: true, relaunch: false, entrypoint: ENTRYPOINT } }, /require streamed file input/],
    [{ fileCache: true }, /require worker OPFS or a trusted cache factory/],
    [{ fileCache: "true" }, /invalid PolkaVM browser file-cache support/],
  ]) {
    const { messages } = await launch({ ...options, expectFailure: true });
    assert.match(messages.find((message) => message.type === "error").message, expected);
  }
});

test("cache precreation limits simultaneous opens and refuses duplicate pending candidates", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  for (const forceInterpreter of BACKENDS) {
    const disks = Array.from({ length: 8 }, () => diskCache(t));
    const opened = deferred();
    const release = deferred();
    t.after(() => release.resolve());
    let opens = 0;
    const live = await launchStream(t,
      Array.from({ length: 7 }, (_, index) => [11, 1, index]),
      forceInterpreter, {
        fileCache: true,
        async createFileCache() {
          const disk = disks[opens++];
          if (opens === 8) opened.resolve();
          await release.promise;
          return disk.cache;
        },
      });
    for (let handle = 2; handle <= 8; handle++) {
      assert.equal((await live.step()).result, handle);
    }
    const send = (handle) => live.receiver.onmessage({
      data: { type: "file-input", handle, name: "source.bin", mimeType: "", file: new DiskBlob(original) },
    });
    for (let handle = 1; handle <= 8; handle++) send(handle);
    await opened.promise;
    const from = live.messages.length;
    for (let repeat = 0; repeat < 16; repeat++) send(1);
    await Promise.resolve();
    assert.equal(opens, 8);
    assert.deepEqual(
      live.messages.slice(from).filter((message) => message.type === "file-input-delivery")
        .map((message) => message.outcome),
      Array(16).fill("refused"),
    );
    live.receiver.onmessage({ data: { type: "stop" } });
    release.resolve();
    await nextMessage(live.messages, from, "terminated");
    assert.ok(disks.every((disk) => disk.state.closes === 1 && !existsSync(disk.path)));
    assert.equal(live.messages.some((message) => message.outcome === "ready"), false);
  }
});

test("both browser cores delete rejected cache candidates before another selection", async (t) => {
  const empty = diskFile(t, new Uint8Array());
  const oversized = diskFile(t, encoder.encode("x"), 0x100000000);
  for (const forceInterpreter of BACKENDS) {
    const disks = [diskCache(t), diskCache(t)];
    let next = 0;
    const live = await launchStream(t, [[0, 1], [0, 1]], forceInterpreter, {
      fileCache: true, createFileCache: async () => disks[next++].cache,
    });
    for (const original of [empty, oversized]) {
      const rejected = await live.select(new DiskBlob(original), "rejected");
      assert.equal(rejected.status, 6);
      assert.equal(rejected.info, 0);
      assert.equal(disks[next - 1].state.closes, 1);
      assert.equal(existsSync(disks[next - 1].path), false);
      assert.deepEqual(original.reads, []);
    }
    await stop(live.receiver, live.messages);
  }
});

test("direct translated cache ownership is independent and closes on refuse, reselect, cancel and stop", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  const live = await translatedStream(t, [
    [5, 1, 4], [6, 1, 0, 4], [7, 1], [8, 1, 0, 4], [3, 1],
    [5, 1, 4], [2, 1],
  ]);
  const sources = Array.from({ length: 4 }, () => diskSource(original));
  const disks = Array.from({ length: 4 }, () => diskCache(t));
  const select = (index) => live.translated.sendFileStream(
    1, "selected.bin", "", 6, sources[index].source, disks[index].cache,
  );
  assert.deepEqual(select(0), { outcome: "ready" });
  assert.deepEqual(select(1), { outcome: "refused" });
  assert.equal(sources[1].state.closes, 1);
  assert.equal(disks[1].state.closes, 1);
  assert.equal(sources[0].state.closes, 0);
  assert.equal(disks[0].state.closes, 0);
  for (const result of [0, 4, 0]) assert.equal(live.step().result, result);
  assert.deepEqual(live.step().bytes, new Uint8Array([0, 1, 2, 3]));
  assert.equal(live.step().status, 2);
  assert.equal(sources[0].state.closes, 1);
  assert.equal(disks[0].state.closes, 1);
  assert.deepEqual(select(2), { outcome: "ready" });
  assert.equal(live.step().result, 0);
  assert.equal(live.step().status, 1);
  assert.equal(sources[2].state.closes, 1);
  assert.equal(disks[2].state.closes, 1);
  assert.deepEqual(select(3), { outcome: "ready" });
  await live.translated.stop();
  assert.equal(sources[3].state.closes, 1);
  assert.equal(disks[3].state.closes, 1);
  assert.equal(decoder.decode(await readFile(original.path)), "source");
});

test("direct translated stop awaits asynchronous disk cleanup and reports its failures", async (t) => {
  const original = diskFile(t, encoder.encode("source"));
  for (const failDeletion of [false, true]) {
    const closing = deferred();
    t.after(() => closing.resolve());
    const live = await translatedStream(t, []);
    const source = diskSource(original);
    const disk = diskCache(t);
    const close = disk.cache.close;
    disk.cache.close = async () => { await closing.promise; close(); };
    live.translated.sendFileStream(1, "selected.bin", "", 6, source.source, disk.cache);
    if (failDeletion) renameSync(disk.path, `${disk.path}.retained`);
    let stopped = false;
    const stopping = live.translated.stop().then((result) => { stopped = true; return result; });
    await Promise.resolve();
    assert.equal(stopped, false);
    assert.equal(source.state.closes, 1);
    closing.resolve();
    assert.deepEqual(await stopping, { cleanupFailed: failDeletion });
    assert.equal(disk.state.closes, 1);
    if (failDeletion) {
      assert.match(live.messages.find((message) => message.type === "error").message, /cleanup failed:.*ENOENT/);
    } else {
      assert.equal(existsSync(disk.path), false);
    }
  }
});
