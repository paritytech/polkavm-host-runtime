import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
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
const BOTH = { inline: true, relaunch: true, entrypoint: ENTRYPOINT };
const BACKENDS = [false, true];
const compiledPrograms = new Map();

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
  records,
  assets = [],
  fileInput = BOTH,
  fileRelaunch,
  forceInterpreter,
  expectFailure = false,
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
  globalThis.createPolkaVmRuntime(receiver);
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      compiledProgram: compiledPrograms.get("file-input"),
      assets: [
        { path: "descriptors", bytes: bytesBuffer(descriptors(records)) },
        ...assets,
      ],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "file-input",
      mediatedInputKinds: ["camera-ur"],
      fileInput: fileInput === "absent" ? undefined : fileInput,
      fileRelaunch,
      forceInterpreter,
    },
  });
  if (expectFailure) {
    await nextMessage(messages, 0, "terminated");
    return { messages, receiver };
  }
  const save = await nextMessage(messages, 0, "save");
  const view = new DataView(save.bytes.buffer, save.bytes.byteOffset);
  const results = Array.from({ length: 16 }, (_, index) =>
    view.getInt32(index * 4, true),
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
      ["absent", [-2, -2]],
      [{ inline: false, relaunch: false, entrypoint: "" }, [-2, -2]],
      [{ inline: true, relaunch: false, entrypoint: ENTRYPOINT }, [1, -4]],
      [{ inline: false, relaunch: true, entrypoint: ENTRYPOINT }, [-4, 1]],
    ]) {
      const launched = await launch({
        records: [INLINE, RELAUNCH],
        fileInput,
        forceInterpreter,
      });
      assert.deepEqual(launched.results.slice(0, 2), expected);
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
