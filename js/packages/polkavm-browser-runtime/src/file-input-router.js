const MAX_FILE_BYTES = {
  inline: 8 * 1024 * 1024,
  relaunch: 128 * 1024 * 1024,
  stream: 0xffffffff,
};
const MAX_REGISTRATIONS = 8;
const MAX_TYPES = 16;
const MAX_PATH_BYTES = 1024;
const encoder = new TextEncoder();
const candidateBindings = new WeakMap();

function fail(message) {
  throw new Error(`file input: ${message}`);
}

function validId(value) {
  return (
    typeof value === "string" &&
    /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/.test(value)
  );
}

function validPath(value) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    encoder.encode(value).byteLength <= MAX_PATH_BYTES &&
    !value.startsWith("/") &&
    !value.includes("\\") &&
    !/[\u0000-\u001f\u007f-\u009f]/.test(value) &&
    value.split("/").every((part) => part && part !== "." && part !== "..")
  );
}

function validMimeType(value) {
  return (
    typeof value === "string" &&
    value.length <= 127 &&
    /^[a-z0-9!#$&^_.+-]+\/[a-z0-9!#$&^_.+-]+$/.test(value)
  );
}

function normalizedHandlers(product) {
  if (!product || typeof product.id !== "string" || product.id.length === 0) {
    fail("product registration requires an id");
  }
  if (!validPath(product.entrypoint)) {
    fail(`${product.id} requires a normalized runtime entrypoint`);
  }
  if (
    !Array.isArray(product.registrations) ||
    product.registrations.length > MAX_REGISTRATIONS ||
    product.registrations.includes(undefined)
  ) {
    fail(`${product.id} has invalid file registrations`);
  }
  const ids = new Set();
  const handles = new Set();
  const mountPaths = new Set();
  return product.registrations.map((registration) => {
    if (
      !registration ||
      typeof registration !== "object" ||
      Array.isArray(registration) ||
      Object.keys(registration).some((key) => !["handle", "descriptor"].includes(key)) ||
      !Number.isInteger(registration.handle) ||
      registration.handle < 1 ||
      registration.handle > 0x7fffffff ||
      handles.has(registration.handle)
    ) {
      fail(`${product.id} has an invalid or duplicate registration handle`);
    }
    handles.add(registration.handle);
    const handler = registration.descriptor;
    if (
      !handler ||
      typeof handler !== "object" ||
      Array.isArray(handler) ||
      Object.keys(handler).some(
        (key) => !["id", "label", "extensions", "mimeTypes", "delivery", "maxBytes", "mountPath"].includes(key),
      ) ||
      !validId(handler.id) ||
      ids.has(handler.id)
    ) {
      fail(`${product.id} has an invalid or duplicate descriptor id`);
    }
    ids.add(handler.id);
    if (
      typeof handler.label !== "string" ||
      handler.label.length === 0 ||
      encoder.encode(handler.label).byteLength > 80
    ) {
      fail(`${product.id}/${handler.id} has an invalid label`);
    }
    const extensions = handler.extensions === undefined ? [] : handler.extensions;
    const mimeTypes = handler.mimeTypes === undefined ? [] : handler.mimeTypes;
    if (
      !Array.isArray(extensions) ||
      !Array.isArray(mimeTypes) ||
      extensions.length > MAX_TYPES ||
      mimeTypes.length > MAX_TYPES ||
      extensions.includes(undefined) ||
      mimeTypes.includes(undefined)
    ) {
      fail(`${product.id}/${handler.id} has invalid accepted types`);
    }
    if (extensions.length === 0 && mimeTypes.length === 0) {
      fail(`${product.id}/${handler.id} declares no accepted type`);
    }
    if (
      new Set(extensions).size !== extensions.length ||
      extensions.some((value) => typeof value !== "string" || !/^\.[a-z0-9]{1,16}$/.test(value))
    ) {
      fail(`${product.id}/${handler.id} has invalid extensions`);
    }
    if (new Set(mimeTypes).size !== mimeTypes.length || !mimeTypes.every(validMimeType)) {
      fail(`${product.id}/${handler.id} has invalid MIME types`);
    }
    if (typeof handler.delivery !== "string" || !Object.hasOwn(MAX_FILE_BYTES, handler.delivery)) {
      fail(`${product.id}/${handler.id} has an invalid delivery mode`);
    }
    if (
      !Number.isSafeInteger(handler.maxBytes) ||
      handler.maxBytes < 1 ||
      handler.maxBytes > MAX_FILE_BYTES[handler.delivery]
    ) {
      fail(`${product.id}/${handler.id} has an invalid maximum size`);
    }
    if (handler.delivery === "relaunch") {
      if (
        !validPath(handler.mountPath) ||
        mountPaths.has(handler.mountPath) ||
        handler.mountPath === product.entrypoint
      ) {
        fail(`${product.id}/${handler.id} has an invalid or duplicate mount path`);
      }
      mountPaths.add(handler.mountPath);
    } else if (Object.hasOwn(handler, "mountPath")) {
      fail(`${product.id}/${handler.id} cannot declare a mount path`);
    }
    const normalized = {
      id: handler.id,
      label: handler.label,
      extensions: Object.freeze([...extensions]),
      mimeTypes: Object.freeze([...mimeTypes]),
      delivery: handler.delivery,
      maxBytes: handler.maxBytes,
    };
    if (handler.delivery === "relaunch") normalized.mountPath = handler.mountPath;
    const candidate = Object.freeze({
      product,
      handle: registration.handle,
      handler: Object.freeze(normalized),
    });
    candidateBindings.set(candidate, {
      registration,
      descriptor: handler,
      id: product.id,
      entrypoint: product.entrypoint,
      signature: JSON.stringify(normalized),
    });
    return candidate;
  });
}

// Handles belong to an execution, not a product id. A replacement registration
// (even with the same handle and descriptor) must receive fresh selection/consent.
function currentCandidate(products, candidate) {
  const binding = candidateBindings.get(candidate);
  const { product } = candidate;
  if (
    !binding ||
    !products.includes(product) ||
    product.id !== binding.id ||
    product.entrypoint !== binding.entrypoint ||
    !product.registrations?.includes(binding.registration) ||
    binding.registration.handle !== candidate.handle ||
    binding.registration.descriptor !== binding.descriptor
  ) {
    return false;
  }
  return normalizedHandlers(product).some(
    (current) => current.handle === candidate.handle &&
      JSON.stringify(current.handler) === binding.signature,
  );
}

function fileMetadata(file) {
  if (
    !file ||
    typeof file.name !== "string" ||
    !Number.isSafeInteger(file.size) ||
    file.size < 1 ||
    (file.type !== undefined && typeof file.type !== "string")
  ) {
    return null;
  }
  const basename = file.name.toWellFormed().split(/[\\/]/).at(-1);
  const sanitized = basename.replace(/[\u0000-\u001f\u007f-\u009f]/g, "�");
  const type = file.type ?? "";
  if (
    !sanitized ||
    encoder.encode(sanitized).byteLength > MAX_PATH_BYTES ||
    (type !== "" && !validMimeType(type.toLowerCase()))
  ) {
    return null;
  }
  return { name: file.name, size: file.size, type };
}

function matches(candidate, metadata) {
  if (!metadata || metadata.size > candidate.handler.maxBytes) return false;
  const basename = metadata.name.split(/[\\/]/).at(-1);
  const dot = basename.lastIndexOf(".");
  const extension = dot < 0 ? "" : basename.slice(dot).toLowerCase();
  const mimeType = metadata.type.toLowerCase();
  return (
    candidate.handler.extensions.includes(extension) ||
    (mimeType !== "" && candidate.handler.mimeTypes.includes(mimeType))
  );
}

/** Matches live runtime file registrations without reading file contents. */
export function routeFileInput(products, file) {
  if (!Array.isArray(products)) fail("products must be an array");
  const metadata = fileMetadata(file);
  return products.flatMap(normalizedHandlers).filter((candidate) => matches(candidate, metadata));
}

/** Returns the browser file-picker accept attribute for all registered products. */
export function filePickerAccept(products) {
  if (!Array.isArray(products)) fail("products must be an array");
  const accepted = new Set();
  for (const { handler } of products.flatMap(normalizedHandlers)) {
    for (const extension of handler.extensions) accepted.add(extension);
    for (const mimeType of handler.mimeTypes) accepted.add(mimeType);
  }
  return [...accepted].join(",");
}

/**
 * Consents a runtime message, never launches a product or mounts an asset.
 * The host binds sendToRuntime to the execution that supplied registrations.
 * Stream delivery retains the original Blob; other modes read bounded bytes.
 */
export function deliverFileInput(options) {
  return deliverToRuntime(options, () => true);
}

async function deliverToRuntime({
  products,
  file,
  chooseCandidate,
  confirmDelivery,
  sendToRuntime,
}, isActive) {
  let candidate;
  try {
    if (!isActive()) return { status: "cancelled" };
    if (typeof confirmDelivery !== "function" || typeof sendToRuntime !== "function") {
      fail("confirmDelivery and sendToRuntime callbacks are required");
    }
    const metadata = fileMetadata(file);
    const candidates = routeFileInput(products, file);
    if (candidates.length === 0) return { status: "unhandled" };
    if (candidates.length > 1) {
      if (typeof chooseCandidate !== "function") return { status: "ambiguous", candidates };
      const selected = await chooseCandidate({ file, candidates });
      if (!isActive() || !selected) return { status: "cancelled" };
      if (!candidates.includes(selected)) fail("chooseCandidate returned an unknown candidate");
      candidate = selected;
    } else {
      candidate = candidates[0];
    }
    const current = () => {
      const latest = fileMetadata(file);
      return isActive() && currentCandidate(products, candidate) &&
        latest?.name === metadata.name && latest.size === metadata.size &&
        latest.type === metadata.type;
    };
    if (!current()) return { status: "cancelled" };
    const stream = candidate.handler.delivery === "stream";
    if (stream ? !(file instanceof Blob) : typeof file.arrayBuffer !== "function") {
      fail(stream ? "stream delivery requires a Blob" : "file requires arrayBuffer()");
    }
    const approved = await confirmDelivery({
      ...candidate,
      file: { ...metadata },
    });
    if (!current()) return { status: "cancelled" };
    if (!approved) return { status: "declined", candidate };
    const message = {
      type: "file-input",
      handle: candidate.handle,
      name: metadata.name,
      mimeType: metadata.type.toLowerCase(),
    };
    if (stream) {
      message.file = file;
    } else {
      const buffer = await file.arrayBuffer();
      if (!current()) return { status: "cancelled" };
      if (!(buffer instanceof ArrayBuffer) || buffer.byteLength !== metadata.size) {
        fail("file size changed while reading");
      }
      message.bytes = new Uint8Array(buffer);
    }
    if (!current()) return { status: "cancelled" };
    await sendToRuntime({ ...candidate, message });
    if (!isActive()) return { status: "cancelled" };
  } catch (error) {
    if (!isActive()) return { status: "cancelled" };
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
  sendToRuntime,
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

  let active = true;
  const deliver = async (file) => {
    const result = await deliverToRuntime({
      products,
      file,
      chooseCandidate,
      confirmDelivery,
      sendToRuntime,
    }, () => active);
    if (active) await onResult(result);
    return result;
  };
  // Event handlers cannot return delivery promises to a caller. Report callback
  // failures explicitly instead of leaving an unhandled rejection.
  const reportError = (error) => {
    if (typeof globalThis.reportError === "function") {
      globalThis.reportError(error);
    } else {
      console.error(error);
    }
  };
  const deliverFromEvent = (file) => {
    void deliver(file).catch(reportError);
  };
  const onClick = () => {
    if (!active) return;
    try {
      picker.accept = filePickerAccept(products);
      picker.click();
    } catch (error) {
      void Promise.resolve().then(() => {
        if (active) return onResult({ status: "rejected", error });
      }).catch(reportError);
    }
  };
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
      active = false;
      openButton.removeEventListener("click", onClick);
      picker.removeEventListener("change", onChange);
      dropTarget.removeEventListener("dragover", onDragOver);
      dropTarget.removeEventListener("drop", onDrop);
      picker.remove();
    },
  };
}
