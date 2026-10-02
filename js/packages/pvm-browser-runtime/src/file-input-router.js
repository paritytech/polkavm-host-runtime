const MAX_FILE_BYTES = 64 * 1024 * 1024;
const MAX_HANDLERS = 16;
const MAX_PATH_BYTES = 1024;

function fail(message) {
  throw new Error(`file input: ${message}`);
}

function validId(value) {
  return (
    typeof value === "string" &&
    /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/.test(value)
  );
}

function normalizedHandlers(product) {
  if (!product || typeof product.id !== "string" || !product.manifest) {
    fail("product registration requires an id and manifest");
  }
  const capability = product.manifest.capabilities?.fileInput;
  if (capability === undefined) return [];
  if (
    !capability ||
    typeof capability !== "object" ||
    Array.isArray(capability) ||
    capability.abiVersion !== 1 ||
    !Array.isArray(capability.handlers) ||
    capability.handlers.length === 0 ||
    capability.handlers.length > MAX_HANDLERS ||
    Object.keys(capability).some(
      (key) => !["abiVersion", "handlers"].includes(key),
    )
  ) {
    fail(`${product.id} has an invalid fileInput capability`);
  }
  const ids = new Set();
  const mountPaths = new Set();
  return capability.handlers.map((handler) => {
    if (
      !handler ||
      typeof handler !== "object" ||
      Array.isArray(handler) ||
      Object.keys(handler).some(
        (key) =>
          ![
            "id",
            "label",
            "extensions",
            "mediaTypes",
            "maxBytes",
            "mountPath",
          ].includes(key),
      ) ||
      !validId(handler.id) ||
      ids.has(handler.id)
    ) {
      fail(`${product.id} has an invalid or duplicate handler id`);
    }
    ids.add(handler.id);
    if (
      typeof handler.label !== "string" ||
      handler.label.trim() === "" ||
      new TextEncoder().encode(handler.label).byteLength > 80
    ) {
      fail(`${product.id}/${handler.id} has an invalid label`);
    }
    const extensions = handler.extensions ?? [];
    const mediaTypes = handler.mediaTypes ?? [];
    if (!Array.isArray(extensions) || !Array.isArray(mediaTypes)) {
      fail(`${product.id}/${handler.id} has invalid accepted types`);
    }
    if (extensions.length === 0 && mediaTypes.length === 0) {
      fail(`${product.id}/${handler.id} declares no accepted type`);
    }
    if (
      new Set(extensions).size !== extensions.length ||
      extensions.some((value) => !/^\.[a-z0-9]{1,16}$/.test(value))
    ) {
      fail(`${product.id}/${handler.id} has invalid extensions`);
    }
    if (
      new Set(mediaTypes).size !== mediaTypes.length ||
      mediaTypes.some(
        (value) =>
          typeof value !== "string" ||
          value.length > 127 ||
          !/^[a-z0-9!#$&^_.+-]+\/[a-z0-9!#$&^_.+-]+$/.test(value),
      )
    ) {
      fail(`${product.id}/${handler.id} has invalid media types`);
    }
    if (
      !Number.isSafeInteger(handler.maxBytes) ||
      handler.maxBytes < 1 ||
      handler.maxBytes > MAX_FILE_BYTES
    ) {
      fail(`${product.id}/${handler.id} has an invalid maximum size`);
    }
    if (
      typeof handler.mountPath !== "string" ||
      handler.mountPath.length === 0 ||
      new TextEncoder().encode(handler.mountPath).byteLength > MAX_PATH_BYTES ||
      handler.mountPath.startsWith("/") ||
      handler.mountPath.includes("\\") ||
      handler.mountPath.split("/").some((part) => !part || part === "." || part === "..") ||
      mountPaths.has(handler.mountPath) ||
      handler.mountPath === product.manifest.runtime?.entrypoint
    ) {
      fail(`${product.id}/${handler.id} has an invalid or duplicate mount path`);
    }
    mountPaths.add(handler.mountPath);
    return {
      product,
      handler: {
        id: handler.id,
        label: handler.label,
        extensions: [...extensions],
        mediaTypes: [...mediaTypes],
        maxBytes: handler.maxBytes,
        mountPath: handler.mountPath,
      },
    };
  });
}

function fileExtension(name) {
  const basename = String(name).split(/[\\/]/).at(-1);
  const dot = basename.lastIndexOf(".");
  return dot < 0 ? "" : basename.slice(dot).toLowerCase();
}

function matches(candidate, file) {
  if (
    !file ||
    typeof file.name !== "string" ||
    !Number.isSafeInteger(file.size) ||
    file.size < 0 ||
    file.size > candidate.handler.maxBytes
  ) {
    return false;
  }
  const extension = fileExtension(file.name);
  const mediaType = typeof file.type === "string" ? file.type.toLowerCase() : "";
  return (
    candidate.handler.extensions.includes(extension) ||
    (mediaType !== "" && candidate.handler.mediaTypes.includes(mediaType))
  );
}

/** Returns validated product/handler candidates without reading file contents. */
export function routeFileInput(products, file) {
  if (!Array.isArray(products)) fail("products must be an array");
  return products.flatMap(normalizedHandlers).filter((candidate) => matches(candidate, file));
}

/** Returns the browser file-picker accept attribute for all registered products. */
export function filePickerAccept(products) {
  const accepted = new Set();
  for (const { handler } of products.flatMap(normalizedHandlers)) {
    for (const extension of handler.extensions) accepted.add(extension);
    for (const mediaType of handler.mediaTypes) accepted.add(mediaType);
  }
  return [...accepted].join(",");
}

/**
 * Resolves a target, asks for consent, then reads and delivers one file.
 * File bytes are deliberately inaccessible to the product before consent.
 * Validation, selection, consent, read, and launch failures return a rejected
 * result with the error and, once selected, the candidate.
 */
export async function deliverFileInput({
  products,
  file,
  chooseCandidate,
  confirmDelivery,
  launchProduct,
}) {
  let candidate;
  try {
    if (typeof confirmDelivery !== "function" || typeof launchProduct !== "function") {
      fail("confirmDelivery and launchProduct callbacks are required");
    }
    const candidates = routeFileInput(products, file);
    if (candidates.length === 0) return { status: "unhandled" };
    if (candidates.length > 1) {
      if (typeof chooseCandidate !== "function") return { status: "ambiguous", candidates };
      const selected = await chooseCandidate({ file, candidates });
      if (!selected) return { status: "cancelled" };
      if (!candidates.includes(selected)) fail("chooseCandidate returned an unknown candidate");
      candidate = selected;
    } else {
      candidate = candidates[0];
    }
    const approved = await confirmDelivery({
      file: { name: file.name, size: file.size, type: file.type || "" },
      product: candidate.product,
      handler: candidate.handler,
    });
    if (!approved) return { status: "declined", candidate };
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (bytes.byteLength !== file.size || bytes.byteLength > candidate.handler.maxBytes) {
      fail("file size changed while reading");
    }
    await launchProduct({
      product: candidate.product,
      handler: candidate.handler,
      asset: { path: candidate.handler.mountPath, bytes },
    });
  } catch (error) {
    return { status: "rejected", candidate, error };
  }
  return { status: "delivered", candidate };
}

/** Wires the same consented delivery path to a drop surface and an Open button. */
export function attachFileInputControls({
  products,
  dropTarget,
  openButton,
  chooseCandidate,
  confirmDelivery,
  launchProduct,
  onResult = () => {},
}) {
  if (!dropTarget?.addEventListener || !openButton?.addEventListener) {
    fail("dropTarget and openButton elements are required");
  }
  const document = openButton.ownerDocument;
  const picker = document.createElement("input");
  picker.type = "file";
  picker.accept = filePickerAccept(products);
  picker.hidden = true;
  document.body.append(picker);

  const deliver = async (file) => {
    const result = await deliverFileInput({
      products,
      file,
      chooseCandidate,
      confirmDelivery,
      launchProduct,
    });
    await onResult(result);
    return result;
  };
  // Event handlers cannot return delivery promises to a caller. Report callback
  // failures explicitly instead of leaving an unhandled rejection.
  const deliverFromEvent = (file) => {
    void deliver(file).catch((error) => {
      if (typeof globalThis.reportError === "function") {
        globalThis.reportError(error);
      } else {
        console.error(error);
      }
    });
  };
  const onClick = () => picker.click();
  const onChange = () => {
    const [file] = picker.files ?? [];
    if (file) deliverFromEvent(file);
    picker.value = "";
  };
  const onDragOver = (event) => {
    const items = [...(event.dataTransfer?.items ?? [])];
    const [file] = event.dataTransfer?.files ?? [];
    if (
      items.some((item) => item.kind === "file") ||
      file
    ) {
      event.preventDefault();
      event.dataTransfer.dropEffect = "copy";
    }
  };
  const onDrop = (event) => {
    const [file] = event.dataTransfer?.files ?? [];
    if (!file) return;
    event.preventDefault();
    deliverFromEvent(file);
  };
  openButton.addEventListener("click", onClick);
  picker.addEventListener("change", onChange);
  dropTarget.addEventListener("dragover", onDragOver);
  dropTarget.addEventListener("drop", onDrop);

  return {
    picker,
    deliver,
    dispose() {
      openButton.removeEventListener("click", onClick);
      picker.removeEventListener("change", onChange);
      dropTarget.removeEventListener("dragover", onDragOver);
      dropTarget.removeEventListener("drop", onDrop);
      picker.remove();
    },
  };
}
