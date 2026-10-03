import assert from "node:assert/strict";
import test from "node:test";

import {
  attachFileInputControls,
  deliverFileInput,
  filePickerAccept,
  routeFileInput,
} from "../src/file-input-router.js";

function product(id, handlers) {
  return {
    id,
    displayName: id === "supafaust" ? "Supafaust" : id,
    manifest: {
      runtime: { entrypoint: "app.polkavm" },
      capabilities: { fileInput: { abiVersion: 1, handlers } },
    },
  };
}

const supafaust = product("supafaust", [
  {
    id: "snes-rom",
    label: "SNES cartridge image",
    extensions: [".sfc", ".smc", ".swc", ".fig"],
    mediaTypes: ["application/x-snes-rom"],
    maxBytes: 16 * 1024 * 1024,
    mountPath: "game/cartridge.sfc",
  },
]);

function file(name = "game.sfc", bytes = new Uint8Array([1, 2, 3])) {
  let reads = 0;
  return {
    name,
    size: bytes.byteLength,
    type: "",
    get reads() {
      return reads;
    },
    async arrayBuffer() {
      reads += 1;
      return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    },
  };
}

test("routes file metadata to matching registered products", () => {
  const candidate = routeFileInput([supafaust], file("Chrono.SFC"))[0];
  assert.equal(candidate.product.id, "supafaust");
  assert.equal(candidate.handler.id, "snes-rom");
  assert.deepEqual(
    filePickerAccept([supafaust]).split(","),
    [".sfc", ".smc", ".swc", ".fig", "application/x-snes-rom"],
  );
  assert.deepEqual(routeFileInput([supafaust], file("game.nes")), []);
});

test("routes files up to the runtime asset ceiling and rejects larger declarations", () => {
  const maxBytes = 128 * 1024 * 1024;
  const handler = { ...supafaust.manifest.capabilities.fileInput.handlers[0], maxBytes };
  const target = product("large-files", [handler]);
  const selected = { name: "large.sfc", size: maxBytes, type: "" };
  assert.deepEqual(
    routeFileInput([target], selected).map(({ product }) => product.id),
    ["large-files"],
  );
  assert.deepEqual(routeFileInput([target], { ...selected, size: maxBytes + 1 }), []);
  handler.maxBytes += 1;
  assert.throws(() => routeFileInput([target], selected), /invalid maximum size/);
});

test("a null optional fileInput capability does not disable other products", async () => {
  const products = [
    { id: "no-files", manifest: { capabilities: { fileInput: null } } },
    supafaust,
  ];
  const selected = file();
  assert.deepEqual(
    routeFileInput(products, selected).map(({ product }) => product.id),
    ["supafaust"],
  );
  assert.equal(filePickerAccept(products), filePickerAccept([supafaust]));
  const result = await deliverFileInput({
    products,
    file: selected,
    confirmDelivery: async () => false,
    launchProduct: async () => assert.fail("declined delivery must not launch"),
  });
  assert.equal(result.status, "declined");
  assert.equal(selected.reads, 0);
});

test("rejects explicit null type lists instead of treating them as omitted", () => {
  for (const field of ["extensions", "mediaTypes"]) {
    const target = structuredClone(supafaust);
    target.manifest.capabilities.fileInput.handlers[0][field] = null;
    assert.throws(() => routeFileInput([target], file()), /invalid accepted types/);
  }
});

test("requires an entrypoint before validating file mount collisions", () => {
  const target = structuredClone(supafaust);
  delete target.manifest.runtime;
  assert.throws(() => routeFileInput([target], file()), /entrypoint/);
  target.manifest.runtime = { entrypoint: "game/cartridge.sfc" };
  assert.throws(() => routeFileInput([target], file()), /mount path/);
});

test("declining consent never reads or delivers file bytes", async () => {
  const selected = file();
  let launches = 0;
  const result = await deliverFileInput({
    products: [supafaust],
    file: selected,
    confirmDelivery: async () => false,
    launchProduct: async () => {
      launches += 1;
    },
  });
  assert.equal(result.status, "declined");
  assert.equal(selected.reads, 0);
  assert.equal(launches, 0);
});

test("confirmed delivery mounts bytes at the registered asset path", async () => {
  const selected = file();
  const launches = [];
  const result = await deliverFileInput({
    products: [supafaust],
    file: selected,
    confirmDelivery: async ({ product: target, handler, file: metadata }) => {
      assert.equal(target.id, "supafaust");
      assert.equal(handler.label, "SNES cartridge image");
      assert.deepEqual(metadata, { name: "game.sfc", size: 3, type: "" });
      return true;
    },
    launchProduct: async (launch) => launches.push(launch),
  });
  assert.equal(result.status, "delivered");
  assert.equal(selected.reads, 1);
  assert.equal(launches[0].asset.path, "game/cartridge.sfc");
  assert.deepEqual([...launches[0].asset.bytes], [1, 2, 3]);
});

test("surfaces a product rejection after consented byte delivery", async () => {
  const selected = file();
  const rejection = new Error("emulator rejected cartridge");
  const result = await deliverFileInput({
    products: [supafaust],
    file: selected,
    confirmDelivery: async () => true,
    launchProduct: async () => {
      throw rejection;
    },
  });
  assert.equal(result.status, "rejected");
  assert.equal(result.error, rejection);
  assert.equal(selected.reads, 1);
});

test("multiple matching products require an explicit target choice", async () => {
  const other = product("other-snes", [
    {
      ...supafaust.manifest.capabilities.fileInput.handlers[0],
      id: "other-rom",
      mountPath: "roms/selected.sfc",
    },
  ]);
  const selected = file();
  const ambiguous = await deliverFileInput({
    products: [supafaust, other],
    file: selected,
    confirmDelivery: async () => true,
    launchProduct: async () => assert.fail("ambiguous file must not launch"),
  });
  assert.equal(ambiguous.status, "ambiguous");
  assert.equal(selected.reads, 0);

  let launched;
  const delivered = await deliverFileInput({
    products: [supafaust, other],
    file: selected,
    chooseCandidate: async ({ candidates }) => candidates[1],
    confirmDelivery: async () => true,
    launchProduct: async (launch) => {
      launched = launch;
    },
  });
  assert.equal(delivered.status, "delivered");
  assert.equal(launched.product.id, "other-snes");
  assert.equal(launched.asset.path, "roms/selected.sfc");
});

function controls(options) {
  class Element extends EventTarget {
    remove() {}
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
    ...attachFileInputControls({
      products: [structuredClone(supafaust)],
      dropTarget,
      openButton,
      confirmDelivery: async () => true,
      launchProduct: async () => {},
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
  test(`${source} delivery reports validation, selection, consent, read and launch errors`, async () => {
    for (const stage of ["validation", "selection", "consent", "read", "size", "launch"]) {
      const rejection = new Error(`${stage} failed`);
      const selected = file();
      const products = [structuredClone(supafaust)];
      let launches = 0;
      if (stage === "selection") {
        products.push({ ...structuredClone(supafaust), id: "other-snes" });
      }
      if (stage === "read") {
        selected.arrayBuffer = async () => { throw rejection; };
      } else if (stage === "size") {
        selected.arrayBuffer = async () => new ArrayBuffer(0);
      }
      const reported = Promise.withResolvers();
      const attached = controls({
        products,
        chooseCandidate: async () => { throw rejection; },
        confirmDelivery: async () => {
          if (stage === "consent") throw rejection;
          return true;
        },
        launchProduct: async () => {
          launches += 1;
          if (stage === "launch") throw rejection;
        },
        onResult: reported.resolve,
      });
      try {
        if (stage === "validation") {
          products[0].manifest.capabilities.fileInput.abiVersion = 2;
        }
        selectFile(attached, source, selected);
        const result = await reported.promise;
        assert.equal(result.status, "rejected");
        if (stage === "validation") {
          assert.match(result.error.message, /invalid fileInput capability/);
        } else if (stage === "size") {
          assert.match(result.error.message, /file size changed/);
        } else {
          assert.equal(result.error, rejection);
        }
        assert.equal(launches, stage === "launch" ? 1 : 0);
        if (["validation", "selection", "consent"].includes(stage)) {
          assert.equal(selected.reads, 0);
        }
        await new Promise((resolve) => setImmediate(resolve));
      } finally {
        attached.dispose();
      }
    }
  });
}

test("DOM delivery observes a rejected result callback without floating a promise", async () => {
  const original = globalThis.reportError;
  const rejection = new Error("result observer failed");
  const reported = Promise.withResolvers();
  globalThis.reportError = reported.resolve;
  const attached = controls({
    onResult: async () => { throw rejection; },
  });
  try {
    selectFile(attached, "drop", file());
    assert.equal(await reported.promise, rejection);
    await new Promise((resolve) => setImmediate(resolve));
  } finally {
    attached.dispose();
    if (original === undefined) {
      delete globalThis.reportError;
    } else {
      globalThis.reportError = original;
    }
  }
});
