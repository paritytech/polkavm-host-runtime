import assert from "node:assert/strict";
import test from "node:test";

import {
  attachFileInputControls,
  deliverFileInput,
  filePickerAccept,
  routeFileInput,
} from "../src/file-input-router.js";

function descriptor(delivery = "relaunch") {
  return {
    id: "snes-rom",
    label: "SNES cartridge image",
    extensions: [".sfc", ".smc", ".swc", ".fig"],
    mimeTypes: ["application/x-snes-rom"],
    delivery,
    maxBytes: delivery === "inline" ? 8 * 1024 * 1024 : 16 * 1024 * 1024,
    ...(delivery === "relaunch" ? { mountPath: "game/cartridge.sfc" } : {}),
  };
}

function product(id = "supafaust", handlers = [descriptor()]) {
  return {
    id,
    displayName: id === "supafaust" ? "Supafaust" : id,
    entrypoint: "app.polkavm",
    registrations: handlers.map((descriptor, index) => ({ handle: index + 1, descriptor })),
  };
}

function file(name = "game.sfc", bytes = new Uint8Array([1, 2, 3])) {
  let reads = 0;
  return {
    name,
    size: bytes.byteLength,
    type: "",
    get reads() { return reads; },
    async arrayBuffer() {
      reads += 1;
      return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    },
  };
}

function streamedFile() {
  const blob = new Blob([new Uint8Array([1, 2, 3])], { type: "application/x-snes-rom" });
  Object.defineProperty(blob, "name", { value: "game.sfc" });
  blob.arrayBuffer = async () => assert.fail("streaming must not read the whole Blob");
  return blob;
}

const mustNotSend = () => assert.fail("file must not reach the runtime");

test("routes runtime registrations and MIME metadata without reading file contents", () => {
  const target = product();
  const selected = file("Chrono.SFC");
  const candidate = routeFileInput([target], selected)[0];
  assert.equal(candidate.product, target);
  assert.equal(candidate.handle, 1);
  assert.deepEqual(candidate.handler, target.registrations[0].descriptor);
  assert.notEqual(candidate.handler, target.registrations[0].descriptor);
  assert.equal(Object.isFrozen(candidate), true);
  assert.equal(Object.isFrozen(candidate.handler.extensions), true);
  assert.equal(selected.reads, 0);
  assert.deepEqual(filePickerAccept([target]).split(","), [
    ".sfc", ".smc", ".swc", ".fig", "application/x-snes-rom",
  ]);
  assert.deepEqual(routeFileInput([target], file("game.nes")), []);
  assert.equal(routeFileInput([target], {
    name: "unknown", size: 3, type: "APPLICATION/X-SNES-ROM",
  }).length, 1);
});

test("only current runtime registrations participate in routing", () => {
  const empty = product("no-files", []);
  empty.fileTypes = [{ extensions: [".sfc"] }];
  assert.deepEqual(routeFileInput([empty], file()), []);
  assert.equal(filePickerAccept([empty]), "");
  const target = product();
  assert.equal(routeFileInput([empty, target], file()).length, 1);
  // Rejection regression: the removed manifest API is not a registration source.
  const obsolete = {
    id: "obsolete",
    manifest: {
      runtime: { entrypoint: "app.polkavm" },
      capabilities: { fileInput: { abiVersion: 1, handlers: [descriptor()] } },
    },
  };
  assert.throws(() => routeFileInput([obsolete], file()), /entrypoint/);
  assert.throws(() => filePickerAccept(null), /products must be an array/);
});

for (const [delivery, maxBytes] of [
  ["inline", 8 * 1024 * 1024],
  ["relaunch", 128 * 1024 * 1024],
  ["stream", 0xffffffff],
]) {
  test(`${delivery} enforces its runtime size ceiling`, () => {
    const handler = { ...descriptor(delivery), maxBytes };
    const target = product("large-files", [handler]);
    const selected = { name: "large.sfc", size: maxBytes, type: "" };
    assert.equal(routeFileInput([target], selected).length, 1);
    assert.deepEqual(routeFileInput([target], { ...selected, size: maxBytes + 1 }), []);
    handler.maxBytes += 1;
    assert.throws(() => routeFileInput([target], selected), /invalid maximum size/);
  });

  test(`${delivery} requires consent before any content handoff`, async () => {
    const selected = delivery === "stream" ? streamedFile() : file();
    const result = await deliverFileInput({
      products: [product("supafaust", [descriptor(delivery)])],
      file: selected,
      confirmDelivery: async () => false,
      sendToRuntime: mustNotSend,
    });
    assert.equal(result.status, "declined");
    if (delivery !== "stream") assert.equal(selected.reads, 0);
  });

  test(`${delivery} sends the runtime file-input message after consent`, async () => {
    const selected = delivery === "stream" ? streamedFile() : file();
    const target = product("supafaust", [descriptor(delivery)]);
    const deliveries = [];
    const result = await deliverFileInput({
      products: [target],
      file: selected,
      confirmDelivery: async ({ product: chosen, handle, handler, file: metadata }) => {
        assert.equal(chosen, target);
        assert.equal(handle, 1);
        assert.equal(handler.delivery, delivery);
        assert.deepEqual(metadata, { name: selected.name, size: 3, type: selected.type });
        if (delivery !== "stream") assert.equal(selected.reads, 0);
        return true;
      },
      sendToRuntime: async (handoff) => deliveries.push(handoff),
    });
    assert.equal(result.status, "delivered");
    assert.equal(deliveries.length, 1);
    const handoff = deliveries[0];
    assert.equal(handoff.product, target);
    assert.equal(handoff.handle, 1);
    assert.equal(handoff.handler.delivery, delivery);
    assert.equal(Object.hasOwn(handoff, "asset"), false);
    const expected = {
      type: "file-input", handle: 1, name: "game.sfc", mimeType: selected.type,
      ...(delivery === "stream" ? { file: selected } : { bytes: new Uint8Array([1, 2, 3]) }),
    };
    assert.deepEqual(handoff.message, expected);
    if (delivery === "stream") {
      assert.equal(handoff.message.file, selected);
      assert.equal(selected instanceof Blob, true);
    } else {
      assert.equal(selected.reads, 1);
    }
  });
}

test("enforces runtime registration handles, counts, ids and mount collisions", () => {
  for (const handle of [0, -1, 1.5, "1", 0x80000000, NaN]) {
    const target = product();
    target.registrations[0].handle = handle;
    assert.throws(() => routeFileInput([target], file()), /registration handle/);
  }
  const handlers = Array.from({ length: 8 }, (_, index) => ({
    ...descriptor("inline"), id: `file-${index}`,
  }));
  const target = product("many", handlers);
  target.registrations[7].handle = 0x7fffffff;
  assert.equal(routeFileInput([target], file()).length, 8);
  target.registrations.push({ handle: 9, descriptor: { ...descriptor("inline"), id: "file-9" } });
  assert.throws(() => routeFileInput([target], file()), /invalid file registrations/);
  for (const mutate of [
    (registrations) => { registrations[1].handle = registrations[0].handle; },
    (registrations) => { registrations[1].descriptor.id = registrations[0].descriptor.id; },
    (registrations) => { registrations[1].descriptor.mountPath = registrations[0].descriptor.mountPath; },
  ]) {
    const target = product("duplicate", [descriptor(), {
      ...descriptor(), id: "other-rom", mountPath: "game/other.sfc",
    }]);
    mutate(target.registrations);
    assert.throws(() => routeFileInput([target], file()), /duplicate/);
  }
});

test("defensively validates descriptor fields and byte quotas", () => {
  const invalid = [
    { id: "a".repeat(65) }, { id: "Uppercase" }, { id: "bad-" },
    { label: "" }, { label: "é".repeat(41) },
    { extensions: null }, { mimeTypes: null },
    { extensions: new Array(1) }, { mimeTypes: new Array(1) },
    { extensions: [], mimeTypes: [] },
    { extensions: [".SFC"] }, { extensions: [".sfc", ".sfc"] },
    { extensions: Array.from({ length: 17 }, (_, index) => `.a${index}`) },
    { mimeTypes: ["application/*"] }, { mimeTypes: ["text/plain", "text/plain"] },
    { mimeTypes: [`text/${"a".repeat(123)}`] },
    { mimeTypes: Array.from({ length: 17 }, (_, index) => `text/a${index}`) },
    { delivery: "asset" }, { delivery: "toString" },
    { delivery: { toString: () => "inline" } },
    { maxBytes: 0 }, { maxBytes: 1.5 }, { maxBytes: NaN },
    { unexpected: true },
  ];
  for (const fields of invalid) {
    const target = product("invalid", [{ ...descriptor(), ...fields }]);
    assert.throws(() => routeFileInput([target], file()), /file input:/);
  }
  const valid = { ...descriptor(), id: "a".repeat(64), label: "é".repeat(40) };
  assert.equal(routeFileInput([product("bounds", [valid])], file()).length, 1);
  for (const field of ["extensions", "mimeTypes"]) {
    const handler = descriptor();
    delete handler[field];
    assert.equal(routeFileInput([product("omitted", [handler])], {
      name: "game.sfc", size: 3, type: "application/x-snes-rom",
    }).length, 1);
  }
});

test("mount paths are normalized, bounded, unique, and relaunch-only", () => {
  for (const mountPath of [
    "", "/game.sfc", "game\\rom.sfc", "./game.sfc", "game/../rom.sfc",
    "game//rom.sfc", "game/", "game\u0000.sfc", "game\u0085.sfc",
    "é".repeat(513), "app.polkavm",
  ]) {
    assert.throws(() => routeFileInput([product("path", [{ ...descriptor(), mountPath }])], file()), /mount path/);
  }
  const target = product("path", [{ ...descriptor(), mountPath: "é".repeat(512) }]);
  assert.equal(routeFileInput([target], file()).length, 1);
  delete target.entrypoint;
  assert.throws(() => routeFileInput([target], file()), /entrypoint/);
  for (const delivery of ["inline", "stream"]) {
    assert.throws(() => routeFileInput([product("path", [{
      ...descriptor(delivery), mountPath: undefined,
    }])], file()), /mount path/);
  }
});

test("invalid and empty files never request consent or reach a runtime", async () => {
  for (const fields of [
    { size: 0 }, { size: -1 }, { size: 1.5 }, { size: NaN },
    { name: "" }, { name: "/" }, { name: `${"é".repeat(512)}.sfc` },
    { type: null }, { type: "invalid" },
  ]) {
    const selected = Object.assign(file(), fields);
    const result = await deliverFileInput({
      products: [product()], file: selected,
      confirmDelivery: () => assert.fail("invalid file must not request consent"),
      sendToRuntime: mustNotSend,
    });
    assert.equal(result.status, "unhandled");
    assert.equal(selected.reads, 0);
  }
  const result = await deliverFileInput({
    products: [product("stream", [descriptor("stream")])], file: file(),
    confirmDelivery: () => assert.fail("stream requires a real Blob"),
    sendToRuntime: mustNotSend,
  });
  assert.equal(result.status, "rejected");
  assert.match(result.error.message, /requires a Blob/);
});

test("ambiguous selections require an exact candidate and support async cancellation", async () => {
  const products = [product(), product("other-snes")];
  const selected = file();
  const options = {
    products, file: selected, confirmDelivery: async () => true, sendToRuntime: mustNotSend,
  };
  assert.equal((await deliverFileInput(options)).status, "ambiguous");
  const entered = Promise.withResolvers();
  const release = Promise.withResolvers();
  const pending = deliverFileInput({
    ...options,
    chooseCandidate: async () => { entered.resolve(); return release.promise; },
  });
  await entered.promise;
  assert.equal(selected.reads, 0);
  release.resolve(null);
  assert.equal((await pending).status, "cancelled");
  const rejected = await deliverFileInput({
    ...options, chooseCandidate: async ({ candidates }) => ({ ...candidates[0] }),
  });
  assert.equal(rejected.status, "rejected");
  assert.match(rejected.error.message, /unknown candidate/);
  let delivered;
  const result = await deliverFileInput({
    ...options,
    chooseCandidate: async ({ candidates }) => candidates[1],
    sendToRuntime: async (handoff) => { delivered = handoff; },
  });
  assert.equal(result.status, "delivered");
  assert.equal(delivered.product, products[1]);
});

const registrationChanges = {
  removed: (products) => { products[0].registrations = []; },
  replaced: (products) => { products[0].registrations = structuredClone(products[0].registrations); },
  descriptor: (products) => {
    products[0].registrations[0].descriptor = { ...products[0].registrations[0].descriptor };
  },
  mutated: (products) => { products[0].registrations[0].descriptor.label = "Different consent"; },
  filters: (products) => { products[0].registrations[0].descriptor.extensions.push(".bin"); },
  handle: (products) => { products[0].registrations[0].handle = 42; },
  execution: (products) => { products[0] = structuredClone(products[0]); },
  identity: (products) => { products[0].id = "replacement"; },
  entrypoint: (products) => { products[0].entrypoint = "replacement.polkavm"; },
};

for (const stage of ["selection", "consent", "read"]) {
  test(`registration changes during asynchronous ${stage} cannot deliver stale bytes`, async () => {
    for (const [change, mutate] of Object.entries(registrationChanges)) {
      const products = [product()];
      if (stage === "selection") products.push(product("other"));
      const selected = file();
      const entered = Promise.withResolvers();
      const release = Promise.withResolvers();
      const pause = async () => { entered.resolve(); await release.promise; };
      if (stage === "read") {
        const read = selected.arrayBuffer;
        selected.arrayBuffer = async () => { await pause(); return read(); };
      }
      let consents = 0;
      const pending = deliverFileInput({
        products, file: selected,
        chooseCandidate: async ({ candidates }) => { await pause(); return candidates[0]; },
        confirmDelivery: async () => {
          consents += 1;
          if (stage === "consent") await pause();
          return true;
        },
        sendToRuntime: mustNotSend,
      });
      await entered.promise;
      mutate(products);
      release.resolve();
      assert.equal((await pending).status, "cancelled", `${stage}/${change}`);
      assert.equal(consents, stage === "selection" ? 0 : 1);
      assert.equal(selected.reads, stage === "read" ? 1 : 0);
    }
  });
}

test("registration array refresh preserves a still-present original registration", async () => {
  const target = product();
  const entered = Promise.withResolvers();
  const release = Promise.withResolvers();
  let sends = 0;
  const pending = deliverFileInput({
    products: [target], file: file(),
    confirmDelivery: async () => { entered.resolve(); return release.promise; },
    sendToRuntime: async () => { sends += 1; },
  });
  await entered.promise;
  target.registrations = [...target.registrations, {
    handle: 2,
    descriptor: { ...descriptor("inline"), id: "other-format", extensions: [".bin"], mimeTypes: [] },
  }];
  release.resolve(true);
  assert.equal((await pending).status, "delivered");
  assert.equal(sends, 1);
});

test("stream selection also binds consent to the original runtime registration", async () => {
  const target = product("stream", [descriptor("stream")]);
  const entered = Promise.withResolvers();
  const release = Promise.withResolvers();
  const pending = deliverFileInput({
    products: [target], file: streamedFile(),
    confirmDelivery: async () => { entered.resolve(); return release.promise; },
    sendToRuntime: mustNotSend,
  });
  await entered.promise;
  target.registrations = structuredClone(target.registrations);
  release.resolve(true);
  assert.equal((await pending).status, "cancelled");
});

test("file metadata changes while consent is pending invalidate consent", async () => {
  for (const field of ["name", "size", "type"]) {
    const selected = file();
    const entered = Promise.withResolvers();
    const release = Promise.withResolvers();
    const pending = deliverFileInput({
      products: [product()], file: selected,
      confirmDelivery: async () => { entered.resolve(); return release.promise; },
      sendToRuntime: mustNotSend,
    });
    await entered.promise;
    selected[field] = { name: "different.sfc", size: 0, type: "text/plain" }[field];
    release.resolve(true);
    assert.equal((await pending).status, "cancelled");
    assert.equal(selected.reads, 0);
  }
});

function controls(options) {
  class Element extends EventTarget {
    click() { this.clicks = (this.clicks ?? 0) + 1; }
    remove() { this.removed = true; }
  }
  const document = {
    body: { append() {} },
    createElement: () => new Element(),
  };
  const dropTarget = new Element();
  const openButton = new Element();
  openButton.ownerDocument = document;
  return {
    dropTarget,
    openButton,
    ...attachFileInputControls({
      products: [product()], dropTarget, openButton,
      confirmDelivery: async () => true,
      sendToRuntime: async () => {},
      ...options,
    }),
  };
}

function selectFile(attached, source, selected) {
  if (source === "picker") {
    attached.picker.files = [selected];
    attached.picker.dispatchEvent(new Event("change"));
  } else {
    const event = new Event("drop", { cancelable: true });
    event.dataTransfer = { files: [selected] };
    attached.dropTarget.dispatchEvent(event);
    assert.equal(event.defaultPrevented, true);
  }
}

for (const source of ["picker", "drop"]) {
  test(`${source} reports validation, selection, consent, read and runtime errors`, async () => {
    for (const stage of ["validation", "selection", "consent", "read", "size", "runtime"]) {
      const rejection = new Error(`${stage} failed`);
      const selected = file();
      const products = [product()];
      let sends = 0;
      if (stage === "selection") products.push(product("other-snes"));
      if (stage === "read") selected.arrayBuffer = async () => { throw rejection; };
      if (stage === "size") selected.arrayBuffer = async () => new ArrayBuffer(0);
      const reported = Promise.withResolvers();
      const attached = controls({
        products,
        chooseCandidate: async () => { throw rejection; },
        confirmDelivery: async () => {
          if (stage === "consent") throw rejection;
          return true;
        },
        sendToRuntime: async () => {
          sends += 1;
          if (stage === "runtime") throw rejection;
        },
        onResult: reported.resolve,
      });
      try {
        if (stage === "validation") products[0].registrations[0].handle = 0;
        selectFile(attached, source, selected);
        const result = await reported.promise;
        assert.equal(result.status, "rejected");
        if (stage === "validation") assert.match(result.error.message, /registration handle/);
        else if (stage === "size") assert.match(result.error.message, /file size changed/);
        else assert.equal(result.error, rejection);
        assert.equal(sends, stage === "runtime" ? 1 : 0);
        if (["validation", "selection", "consent"].includes(stage)) assert.equal(selected.reads, 0);
        await new Promise((resolve) => setImmediate(resolve));
      } finally {
        attached.dispose();
      }
    }
  });
}

for (const stage of ["selection", "consent", "read", "stream-consent"]) {
  test(`controls disposal cancels an asynchronous ${stage} and suppresses result callbacks`, async () => {
    const stream = stage === "stream-consent";
    const products = [product("supafaust", [descriptor(stream ? "stream" : "relaunch")])];
    if (stage === "selection") products.push(product("other"));
    const selected = stream ? streamedFile() : file();
    const entered = Promise.withResolvers();
    const release = Promise.withResolvers();
    const pause = async () => { entered.resolve(); await release.promise; };
    if (stage === "read") {
      const read = selected.arrayBuffer;
      selected.arrayBuffer = async () => { await pause(); return read(); };
    }
    const attached = controls({
      products,
      chooseCandidate: async ({ candidates }) => { await pause(); return candidates[0]; },
      confirmDelivery: async () => {
        if (stage === "consent" || stream) await pause();
        return true;
      },
      sendToRuntime: mustNotSend,
      onResult: () => assert.fail("disposed controls must not report a late result"),
    });
    const pending = attached.deliver(selected);
    await entered.promise;
    attached.dispose();
    release.resolve();
    assert.equal((await pending).status, "cancelled");
    assert.equal((await attached.deliver(selected)).status, "cancelled");
    if (!stream) assert.equal(selected.reads, stage === "read" ? 1 : 0);
    assert.equal(attached.picker.removed, true);
    attached.openButton.dispatchEvent(new Event("click"));
    assert.equal(attached.picker.clicks, undefined);
  });
}

test("picker accept refreshes from live runtime registrations and reports invalid updates", async () => {
  const target = product();
  const reported = Promise.withResolvers();
  const attached = controls({ products: [target], onResult: reported.resolve });
  try {
    target.registrations = [{ handle: 2, descriptor: {
      ...descriptor("inline"), extensions: [".bin"], mimeTypes: [],
    } }];
    attached.openButton.dispatchEvent(new Event("click"));
    assert.equal(attached.picker.accept, ".bin");
    assert.equal(attached.picker.clicks, 1);
    target.registrations[0].handle = 0;
    attached.openButton.dispatchEvent(new Event("click"));
    const result = await reported.promise;
    assert.equal(result.status, "rejected");
    assert.match(result.error.message, /registration handle/);
  } finally {
    attached.dispose();
  }
});

test("DOM delivery observes a rejected result callback without floating a promise", async () => {
  const original = globalThis.reportError;
  const rejection = new Error("result observer failed");
  const reported = Promise.withResolvers();
  globalThis.reportError = reported.resolve;
  const attached = controls({ onResult: async () => { throw rejection; } });
  try {
    selectFile(attached, "drop", file());
    assert.equal(await reported.promise, rejection);
    await new Promise((resolve) => setImmediate(resolve));
  } finally {
    attached.dispose();
    if (original === undefined) delete globalThis.reportError;
    else globalThis.reportError = original;
  }
});
