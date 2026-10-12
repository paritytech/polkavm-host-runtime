/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

"use strict";

(() => {
  const STATUS_FINISHED = -1;
  const STATUS_ECALL = -2;
  const STATUS_TRAP = -3;
  const STATUS_OUT_OF_GAS = -4;
  const CORE_STATUS_INVALID = -3;
  const CORE_STATUS_DENIED = -5;
  const CORE_STATUS_LIMIT = -6;
  const INPUT_EVENT_BYTES = 8;
  const MOTION_SAMPLE_BYTES = 48;
  const MOTION_STATUS_UNAVAILABLE = 0;
  const MOTION_STATUS_AVAILABLE = 1;
  const MOTION_STATUS_PERMISSION_DENIED = 2;
  const MOTION_ERROR_UNAVAILABLE = -1;
  const MOTION_ERROR_PERMISSION_DENIED = -2;
  const MOTION_ERROR_INVALID_GUEST_RANGE = -3;
  const MOTION_ERROR_BUFFER_TOO_SMALL = -4;
  const MAX_MEDIATED_INPUT_KIND_BYTES = 32;
  const MAX_MEDIATED_INPUT_MEDIA_TYPE_BYTES = 64;
  const MAX_MEDIATED_INPUT_BYTES = 1024 * 1024;
  const MAX_MEDIATED_INPUT_REGISTRATIONS = 8;
  const MEDIATED_INPUT_STATUS_REGISTERED = 1;
  const MEDIATED_INPUT_STATUS_ACTIVE = 2;
  const MEDIATED_INPUT_STATUS_READY = 3;
  const MEDIATED_INPUT_STATUS_CANCELLED = 4;
  const MEDIATED_INPUT_STATUS_PERMISSION_DENIED = 5;
  const MEDIATED_INPUT_STATUS_FAILED = 6;
  const INPUT_POINTER_CAPTURE = 15;
  const INPUT_SAFE_AREA_INSETS = 16;
  const INPUT_KEYBOARD_INSETS = 17;
  const INPUT_INSETS_HORIZONTAL = 0;
  const INPUT_INSETS_VERTICAL = 1;
  const POINTER_CAPTURE_IMPORT = "host_pointer_capture";
  const UPDATE_AFTER_IMPORT = "host_update_after";
  const POINTER_CAPTURE_RELEASE = 0;
  const POINTER_CAPTURE_ARM = 1;
  const POINTER_CAPTURE_RELEASED = 0;
  const POINTER_CAPTURE_ARMED = 1;
  const POINTER_CAPTURE_ACTIVE = 2;
  const POINTER_CAPTURE_UNSUPPORTED = -1;
  const POINTER_CAPTURE_INVALID_REQUEST = -2;
  const MAX_INPUT_EVENTS = 4096;
  const MAX_HOSTCALLS_PER_INIT = 1024 * 1024;
  const MAX_HOSTCALLS_PER_UPDATE = 65536;
  const MAX_HOSTCALL_BYTES = 32 * 1024 * 1024;
  const MAX_CORE_RANDOM_BYTES = 4 * 1024;
  const MAX_LOG_BYTES = 4 * 1024;
  const MAX_SAVE_BYTES = 1024 * 1024;
  const MAX_RANDOM_BYTES_PER_CALL = 4 * 1024;
  const RANDOM_BYTES_PER_EXECUTION = 64 * 1024;
  const MAX_AUDIO_SAMPLES = 48000 * 2;
  const MAX_FRAME_BYTES = 16 * 1024 * 1024;
  const MAX_TRI2D_BYTES = 8 * 1024 * 1024;
  const MAX_UI_SEMANTICS_BYTES = 256 * 1024;
  const MAX_UI_SEMANTIC_NODES = 1024;
  const MAX_UI_SEMANTIC_STRING_BYTES = 1024;
  const UI_OUTPUT_HEADER_BYTES = 48;
  const UI_OUTPUT_COMMAND_HEADER_BYTES = 8;
  const MAX_UI_COPY_IMAGE_BYTES = 4 * 1024 * 1024;
  const MAX_UI_OUTPUT_BYTES = MAX_UI_COPY_IMAGE_BYTES + 256 * 1024;
  const MAX_UI_OUTPUT_COMMANDS = 64;
  const MAX_UI_COPY_TEXT_BYTES = 64 * 1024;
  const MAX_UI_COPY_IMAGE_PIXELS = 1024 * 1024;
  const MAX_UI_COPY_IMAGE_DIMENSION = 2048;
  const MAX_UI_OPEN_URL_BYTES = 8 * 1024;
  const UI_CURSOR_ICONS = Object.freeze([
    "default",
    "none",
    "context-menu",
    "help",
    "pointer",
    "progress",
    "wait",
    "cell",
    "crosshair",
    "text",
    "vertical-text",
    "alias",
    "copy",
    "move",
    "no-drop",
    "not-allowed",
    "grab",
    "grabbing",
    "all-scroll",
    "ew-resize",
    "nesw-resize",
    "nwse-resize",
    "ns-resize",
    "e-resize",
    "se-resize",
    "s-resize",
    "sw-resize",
    "w-resize",
    "nw-resize",
    "n-resize",
    "ne-resize",
    "col-resize",
    "row-resize",
    "zoom-in",
    "zoom-out",
  ]);
  const MAX_GPU_BATCH_BYTES = 4 * 1024 * 1024;
  const MAX_GPU_EVENT_BYTES = 64 * 1024;
  const MAX_GPU_EVENTS = 256;
  const MAX_GPU_SUBMITS_PER_UPDATE = 8;
  const MAX_HOST_FRAME_BYTES = 1024 * 1024;
  const MAX_HOST_FRAMES = 32;
  const MAX_QUEUED_HOST_FRAME_BYTES = 4 * 1024 * 1024;
  const MAX_GPU_COMMANDS = 16_384;
  const GPU_ERROR_MALFORMED_BATCH = -2;
  const GPU_ERROR_QUOTA_EXCEEDED = -3;
  const GPU_ERROR_INVALID_STATE = -5;
  const IOV_MAX = 1024n;
  const AT_FDCWD = BigInt.asUintN(64, -100n);
  const ENOSYS = 38;
  const EFAULT = 14;
  const ENOENT = 2;
  const EBADF = 9;
  const EACCES = 13;
  const EINVAL = 22;
  const SYS_OPENAT = 56n;
  const SYS_CLOSE = 57n;
  const SYS_LSEEK = 62n;
  const SYS_READ = 63n;
  const SYS_READV = 65n;
  const SYS_WRITEV = 66n;
  const SYS_EXIT = 93n;
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  const strictDecoder = new TextDecoder("utf-8", { fatal: true });

  function executionRandomBytes() {
    const bytes = new Uint8Array(RANDOM_BYTES_PER_EXECUTION);
    const browserCrypto = globalThis.crypto;
    if (typeof browserCrypto?.getRandomValues !== "function") {
      return new Uint8Array();
    }
    try {
      browserCrypto.getRandomValues(bytes);
      return bytes;
    } catch {
      return new Uint8Array();
    }
  }

  function isWebGpuProfile(profile) {
    return profile === "webgpu-raster" || profile === "webgpu";
  }

  function validMediatedInputToken(value, maxBytes) {
    const bytes = encoder.encode(value);
    return (
      bytes.byteLength > 0 &&
      bytes.byteLength <= maxBytes &&
      /^[a-z0-9][a-z0-9+._-]*[a-z0-9]$|^[a-z0-9]$/.test(value)
    );
  }

  function isComputeOpcode(opcode) {
    return opcode >= 24 && opcode <= 29;
  }

  function validUiSemantics(bytes) {
    let snapshot;
    try {
      snapshot = JSON.parse(decoder.decode(bytes));
    } catch {
      return false;
    }
    if (
      snapshot?.version !== 1 ||
      !Number.isSafeInteger(snapshot.generation) ||
      snapshot.generation < 0 ||
      !Array.isArray(snapshot.nodes) ||
      !snapshot.nodes.length ||
      snapshot.nodes.length > MAX_UI_SEMANTIC_NODES
    ) {
      return false;
    }
    const ids = new Set();
    let roots = 0;
    for (const node of snapshot.nodes) {
      if (
        typeof node?.id !== "string" ||
        !/^[0-9a-f]{1,16}$/.test(node.id) ||
        ids.has(node.id) ||
        !Array.isArray(node.bounds) ||
        node.bounds.length !== 4 ||
        !node.bounds.every(Number.isFinite) ||
        node.bounds[2] < node.bounds[0] ||
        node.bounds[3] < node.bounds[1] ||
        typeof node.name !== "string" ||
        encoder.encode(node.name).byteLength > MAX_UI_SEMANTIC_STRING_BYTES ||
        typeof node.value !== "string" ||
        encoder.encode(node.value).byteLength > MAX_UI_SEMANTIC_STRING_BYTES
      ) {
        return false;
      }
      ids.add(node.id);
      if (node.parent === null) {
        roots++;
      }
    }
    return (
      roots === 1 &&
      snapshot.nodes.every(
        (node) => node.parent === null || ids.has(node.parent),
      )
    );
  }

  function decodeUiOutput(bytes) {
    if (
      !(bytes instanceof Uint8Array) ||
      bytes.byteLength < UI_OUTPUT_HEADER_BYTES ||
      bytes.byteLength > MAX_UI_OUTPUT_BYTES
    ) {
      return null;
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    if (
      bytes[0] !== 0x50 ||
      bytes[1] !== 0x55 ||
      bytes[2] !== 0x49 ||
      bytes[3] !== 0x31 ||
      view.getUint16(4, true) !== 1 ||
      view.getUint16(6, true) !== UI_OUTPUT_HEADER_BYTES ||
      view.getUint32(8, true) !== bytes.byteLength
    ) {
      return null;
    }
    const commandCount = view.getUint16(12, true);
    const cursorIcon = UI_CURSOR_ICONS[bytes[14]];
    const flags = bytes[15];
    if (
      commandCount > MAX_UI_OUTPUT_COMMANDS ||
      cursorIcon === undefined ||
      (flags & ~3) !== 0
    ) {
      return null;
    }
    const readRect = (offset) => [
      view.getFloat32(offset, true),
      view.getFloat32(offset + 4, true),
      view.getFloat32(offset + 8, true),
      view.getFloat32(offset + 12, true),
    ];
    const validRect = (rect) =>
      rect.every(Number.isFinite) && rect[2] >= rect[0] && rect[3] >= rect[1];
    let ime = null;
    if (flags & 2) {
      const rect = readRect(16);
      const cursorRect = readRect(32);
      if (!validRect(rect) || !validRect(cursorRect)) {
        return null;
      }
      ime = { rect, cursorRect };
    } else if (
      bytes.subarray(16, UI_OUTPUT_HEADER_BYTES).some((byte) => byte)
    ) {
      return null;
    }

    const commands = [];
    let offset = UI_OUTPUT_HEADER_BYTES;
    try {
      for (let index = 0; index < commandCount; index++) {
        if (offset + UI_OUTPUT_COMMAND_HEADER_BYTES > bytes.byteLength) {
          return null;
        }
        const opcode = bytes[offset];
        const commandFlags = bytes[offset + 1];
        if (bytes[offset + 2] !== 0 || bytes[offset + 3] !== 0) {
          return null;
        }
        const payloadLength = view.getUint32(offset + 4, true);
        const payloadStart = offset + UI_OUTPUT_COMMAND_HEADER_BYTES;
        const payloadEnd = payloadStart + payloadLength;
        if (payloadEnd > bytes.byteLength) {
          return null;
        }
        const payloadBytes = bytes.subarray(payloadStart, payloadEnd);
        if (opcode === 1) {
          if (commandFlags !== 0 || payloadLength > MAX_UI_COPY_TEXT_BYTES) {
            return null;
          }
          commands.push({
            type: "copy-text",
            text: strictDecoder.decode(payloadBytes),
          });
        } else if (opcode === 2) {
          if (
            (commandFlags & ~1) !== 0 ||
            payloadLength === 0 ||
            payloadLength > MAX_UI_OPEN_URL_BYTES
          ) {
            return null;
          }
          commands.push({
            type: "open-url",
            url: strictDecoder.decode(payloadBytes),
            newSurface: (commandFlags & 1) !== 0,
          });
        } else if (opcode === 3) {
          if (commandFlags !== 0 || payloadLength < 8) {
            return null;
          }
          const payloadView = new DataView(
            bytes.buffer,
            bytes.byteOffset + payloadStart,
            payloadLength,
          );
          const width = payloadView.getUint32(0, true);
          const height = payloadView.getUint32(4, true);
          const pixelBytes = payloadLength - 8;
          if (
            width === 0 ||
            height === 0 ||
            width > MAX_UI_COPY_IMAGE_DIMENSION ||
            height > MAX_UI_COPY_IMAGE_DIMENSION ||
            width * height > MAX_UI_COPY_IMAGE_PIXELS ||
            width * height * 4 !== pixelBytes
          ) {
            return null;
          }
          commands.push({
            type: "copy-image",
            width,
            height,
            rgba: payloadBytes.subarray(8),
          });
        } else {
          return null;
        }
        offset = payloadEnd;
      }
    } catch {
      return null;
    }
    if (offset !== bytes.byteLength) {
      return null;
    }
    return {
      cursorIcon,
      mutableTextUnderCursor: (flags & 1) !== 0,
      ime,
      commands,
    };
  }

  function readMetadata(module) {
    const sections = WebAssembly.Module.customSections(
      module,
      "epoca.pvm.meta",
    );
    if (sections.length !== 1) {
      throw new Error("translated PolkaVM module has invalid metadata");
    }
    const bytes = new Uint8Array(sections[0]);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let offset = 0;
    const requireBytes = (length) => {
      if (offset + length > bytes.byteLength) {
        throw new Error("translated PolkaVM metadata is truncated");
      }
    };
    const readU16 = () => {
      requireBytes(2);
      const value = view.getUint16(offset, true);
      offset += 2;
      return value;
    };
    const readU32 = () => {
      requireBytes(4);
      const value = view.getUint32(offset, true);
      offset += 4;
      return value;
    };
    const readString = (length) => {
      requireBytes(length);
      const value = decoder.decode(bytes.subarray(offset, offset + length));
      offset += length;
      return value;
    };
    requireBytes(4);
    if (decoder.decode(bytes.subarray(0, 4)) !== "EPM2") {
      throw new Error("translated PolkaVM metadata has an incompatible ABI");
    }
    offset = 4;
    const is64Bit = readU32() !== 0;
    const names = [
      "roAddress",
      "roSize",
      "roPhysical",
      "rwAddress",
      "rwSize",
      "rwPhysical",
      "heapBase",
      "heapLimit",
      "stackLow",
      "stackHigh",
      "stackPhysical",
    ];
    const layout = {};
    for (const name of names) {
      layout[name] = readU32();
    }
    const imports = [];
    const importCount = readU32();
    for (let index = 0; index < importCount; index++) {
      const length = readU16();
      imports.push(length ? readString(length) : null);
    }
    const exports = new Map();
    const exportCount = readU32();
    for (let index = 0; index < exportCount; index++) {
      const name = readString(readU16());
      const block = readU32();
      if (!name || exports.has(name)) {
        throw new Error("translated PolkaVM metadata has invalid exports");
      }
      exports.set(name, block);
    }
    if (offset !== bytes.byteLength) {
      throw new Error("translated PolkaVM metadata has trailing data");
    }
    return { is64Bit, layout, imports, exports };
  }

  function errno(code) {
    return BigInt.asUintN(64, -BigInt(code));
  }

  function normalizedPath(path) {
    while (path.startsWith("./")) {
      path = path.slice(2);
    }
    return path.replace(/^\/+/, "");
  }

  function hidToCoreVm(code) {
    if (code >= 0x04 && code <= 0x1d) {
      return 0x61 + code - 0x04;
    }
    if (code >= 0x1e && code <= 0x26) {
      return 0x31 + code - 0x1e;
    }
    const keys = new Map([
      [0x27, 0x30],
      [0x28, 0x0a],
      [0x58, 0x0a],
      [0x29, 0x1b],
      [0x2a, 0x08],
      [0x2b, 0x09],
      [0x2c, 0x20],
      [0x2d, 0x2d],
      [0x56, 0x2d],
      [0x2e, 0x3d],
      [0x2f, 0x5b],
      [0x30, 0x5d],
      [0x31, 0x5c],
      [0x33, 0x3b],
      [0x34, 0x27],
      [0x35, 0x60],
      [0x36, 0x2c],
      [0x37, 0x2e],
      [0x63, 0x2e],
      [0x38, 0x2f],
      [0x54, 0x2f],
      [0x46, 0x91],
      [0x47, 0x92],
      [0x48, 0x93],
      [0x49, 0x94],
      [0x4a, 0x96],
      [0x4b, 0x98],
      [0x4c, 0x95],
      [0x4d, 0x97],
      [0x4e, 0x99],
      [0x4f, 0x82],
      [0x50, 0x83],
      [0x51, 0x81],
      [0x52, 0x80],
      [0x55, 0x2a],
      [0x57, 0x2b],
      [0x59, 0x97],
      [0x5a, 0x81],
      [0x5b, 0x99],
      [0x5c, 0x83],
      [0x5d, 0x35],
      [0x5e, 0x82],
      [0x5f, 0x96],
      [0x60, 0x80],
      [0x61, 0x98],
      [0x62, 0x2e],
      [0xe0, 0x9c],
      [0xe1, 0x9a],
      [0xe2, 0x9e],
      [0xe4, 0x9d],
      [0xe5, 0x9b],
      [0xe6, 0x9f],
    ]);
    if (code >= 0x3a && code <= 0x45) {
      return 0x84 + code - 0x3a;
    }
    return keys.get(code);
  }

  function validMotionSample(bytes) {
    if (
      !(bytes instanceof Uint8Array) ||
      bytes.byteLength !== MOTION_SAMPLE_BYTES
    ) {
      return false;
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const flags = view.getUint16(6, true);
    if (
      bytes[0] !== 0x50 ||
      bytes[1] !== 0x4d ||
      bytes[2] !== 0x4f ||
      bytes[3] !== 0x31 ||
      view.getUint16(4, true) !== 1 ||
      !flags ||
      flags & ~7 ||
      (flags & 4 && !(flags & 2)) ||
      view.getUint32(8, true) !== MOTION_SAMPLE_BYTES ||
      view.getUint32(12, true) === 0
    ) {
      return false;
    }
    const timestamp = view.getFloat64(16, true);
    if (!Number.isFinite(timestamp) || timestamp < 0) {
      return false;
    }
    for (let offset = 24; offset < MOTION_SAMPLE_BYTES; offset += 4) {
      if (!Number.isFinite(view.getFloat32(offset, true))) {
        return false;
      }
    }
    return true;
  }

  class TranslatedPolkaVmRuntime {
    static async compile(bytes) {
      const module = await WebAssembly.compile(bytes);
      const parts = [];
      for (const bytes of WebAssembly.Module.customSections(
        module,
        "epoca.pvm.code-part",
      )) {
        // Keep native compilation bounded to one code part at a time.
        parts.push(await WebAssembly.compile(bytes));
      }
      return { module, parts };
    }

    static isCompiledProgram(value) {
      if (
        value === null ||
        typeof value !== "object" ||
        !(value.module instanceof WebAssembly.Module) ||
        !Array.isArray(value.parts)
      ) {
        return false;
      }
      for (const part of value.parts) {
        if (!(part instanceof WebAssembly.Module)) {
          return false;
        }
      }
      return true;
    }

    constructor(
      program,
      assets,
      emit,
      maxGas,
      audioEnabled,
      graphicsProfile,
      gpuCapabilities = null,
      motionAvailability = MOTION_STATUS_UNAVAILABLE,
      mediatedInputKinds = [],
    ) {
      if (!TranslatedPolkaVmRuntime.isCompiledProgram(program)) {
        throw new TypeError("invalid translated PolkaVM compiled program");
      }
      this.metadata = readMetadata(program.module);
      this.instance = new WebAssembly.Instance(program.module, {});
      this.pvm = this.instance.exports;
      this.memory = this.pvm.memory;
      if (!(this.memory instanceof WebAssembly.Memory)) {
        throw new Error("translated PolkaVM module is missing guest memory");
      }
      const imports = { pvm: this.pvm };
      this.partInstances = program.parts.map(
        (part) => new WebAssembly.Instance(part, imports),
      );
      this.assets = new Map(
        assets.map((asset) => [
          normalizedPath(asset.path),
          new Uint8Array(asset.bytes),
        ]),
      );
      this.emit = emit;
      this.audioEnabled = audioEnabled;
      if (
        !["framebuffer", "tri2d", "webgpu-raster", "webgpu"].includes(
          graphicsProfile,
        )
      ) {
        throw new Error(
          `translated PolkaVM runtime has invalid graphics profile ${graphicsProfile}`,
        );
      }
      this.graphicsProfile = graphicsProfile;
      if (
        isWebGpuProfile(graphicsProfile) &&
        !(gpuCapabilities instanceof Uint8Array)
      ) {
        throw new Error(
          "WebGPU capabilities are required before PolkaVM initialization",
        );
      }
      this.gpuCapabilities =
        gpuCapabilities instanceof Uint8Array ? gpuCapabilities.slice() : null;
      this.gpuEvents = [];
      this.gpuSubmits = 0;
      this.gpuLastSequence = 0n;
      this.hostFrameRequests = 0;
      this.hostFrameRequestBytes = 0;
      this.hostFrameResponses = [];
      this.hostFrameResponseBytes = 0;
      if (
        !Array.isArray(mediatedInputKinds) ||
        mediatedInputKinds.length > MAX_MEDIATED_INPUT_REGISTRATIONS ||
        mediatedInputKinds.some(
          (kind) =>
            typeof kind !== "string" ||
            !validMediatedInputToken(kind, MAX_MEDIATED_INPUT_KIND_BYTES),
        ) ||
        new Set(mediatedInputKinds).size !== mediatedInputKinds.length
      ) {
        throw new Error("translated PolkaVM runtime has invalid mediated-input kinds");
      }
      this.mediatedInputKinds = new Set(mediatedInputKinds);
      this.mediatedInputRegistrations = new Map();
      this.nextMediatedInputHandle = 0;
      this.activeMediatedInputHandle = null;
      this.tri2dSubmitted = false;
      this.uiSemanticsSubmitted = false;
      this.uiOutputSubmitted = false;
      this.maxGas = BigInt(maxGas);
      this.input = [];
      this.coreInput = [];
      this.epocaInput = [];
      this.pointer = null;
      this.setMotionAvailability(motionAvailability);
      this.motionSample = null;
      this.pointerCapture = {
        supported: false,
        armed: false,
        active: false,
        request: null,
      };
      this.timeMs = null;
      this.randomBytes = executionRandomBytes();
      this.randomOffset = 0;
      this.updateAfterMs = null;
      this.clockStartedAt = performance.now();
      this.hostcalls = 0;
      this.hostcallBytes = 0;
      this.resumePending = false;
      this.stopped = false;
      this.coreVm = this.metadata.exports.has("_pvm_start");
      if (this.coreVm && graphicsProfile !== "framebuffer") {
        throw new Error(
          "CoreVM guests require the framebuffer graphics profile",
        );
      }
      this.coreVmStarted = false;
      this.palette = new Uint32Array(256);
      this.palette.fill(0xffffffff);
      this.audioChannels = 0;
      this.audioSampleRate = 0;
      this.fds = new Map();
      this.nextFd = 3;
      this.imports = this.metadata.imports;
      this.exports = this.metadata.exports;
      for (let index = 0; index < 13; index++) {
        if (!(this.pvm[`r${index}`] instanceof WebAssembly.Global)) {
          throw new Error("translated PolkaVM module is missing registers");
        }
      }
    }

    initialize(maxGas = this.maxGas) {
      const gas = BigInt(maxGas);
      this.#resetBudget(MAX_HOSTCALLS_PER_INIT, gas);
      if (this.coreVm) {
        this.#setupCoreVm();
        return;
      }
      const init = this.exports.get("init");
      if (init !== undefined) {
        this.#run(init, false, gas);
      }
    }

    usesMotion() {
      return this.imports.includes("host_motion_read");
    }

    usesPointerCapture() {
      return this.imports.includes(POINTER_CAPTURE_IMPORT);
    }

    usesUpdateScheduling() {
      return this.imports.includes(UPDATE_AFTER_IMPORT);
    }

    updateAfterMilliseconds() {
      return this.updateAfterMs;
    }

    setPointerCaptureSupported(supported) {
      this.pointerCapture.supported = supported === true;
      if (!this.pointerCapture.supported) {
        this.pointerCapture.armed = false;
        this.pointerCapture.request = null;
      }
    }

    setPointerCaptureActive(active) {
      const next = active === true;
      if (this.pointerCapture.active === next) {
        return;
      }
      this.pointerCapture.active = next;
      if (next) {
        this.pointerCapture.armed = false;
      }
      const record = new Uint8Array(INPUT_EVENT_BYTES);
      record[0] = INPUT_POINTER_CAPTURE;
      record[1] = next ? 1 : 0;
      this.sendInput(record);
    }

    takePointerCaptureRequest() {
      const request = this.pointerCapture.request;
      this.pointerCapture.request = null;
      return request;
    }

    #requestPointerCapture(request) {
      const state = this.pointerCapture;
      if (!state.supported) {
        return POINTER_CAPTURE_UNSUPPORTED;
      }
      if (request === POINTER_CAPTURE_ARM) {
        state.armed = true;
        state.request = true;
      } else if (request === POINTER_CAPTURE_RELEASE) {
        state.armed = false;
        state.request = false;
      } else {
        return POINTER_CAPTURE_INVALID_REQUEST;
      }
      if (state.active) {
        return POINTER_CAPTURE_ACTIVE;
      }
      return state.armed ? POINTER_CAPTURE_ARMED : POINTER_CAPTURE_RELEASED;
    }

    update(timeMs) {
      if (this.stopped) {
        return;
      }
      this.timeMs = timeMs;
      this.updateAfterMs = null;
      this.gpuSubmits = 0;
      this.hostFrameRequests = 0;
      this.uiSemanticsSubmitted = false;
      this.uiOutputSubmitted = false;
      this.hostFrameRequestBytes = 0;
      this.#resetBudget(
        this.coreVm && !this.coreVmStarted
          ? MAX_HOSTCALLS_PER_INIT
          : MAX_HOSTCALLS_PER_UPDATE,
      );
      if (this.coreVm) {
        this.#run(this.exports.get("_pvm_start"), true);
        this.coreVmStarted = true;
        return;
      }
      const update = this.exports.get("update");
      if (update === undefined) {
        throw new Error("translated PolkaVM guest has no update export");
      }
      this.#run(update, false);
    }

    pauseInput() {
      const survivesPause = (record) =>
        record[0] === 2 || record[0] === 4 || record[0] === 7 ||
        record[0] === 12 || record[0] === 16 || record[0] === 17 ||
        record[0] === 20 || record[0] === 21 ||
        ((record[0] === 13 || record[0] === 15) && record[1] === 0);
      this.input = this.input.filter(survivesPause);
      this.epocaInput = this.epocaInput.filter(survivesPause);
      this.coreInput = this.coreInput.filter(
        ([key, value]) => value === 0 && key !== 0xa3 && key !== 0xa4,
      );
      this.pointer = null;
      this.motionSample = null;
    }

    sendInput(bytes) {
      if (this.stopped || bytes.byteLength !== INPUT_EVENT_BYTES) {
        return;
      }
      if (!this.coreVm) {
        const copy = bytes.slice();
        const type = copy[0];
        if (type === 7) {
          const existing = this.input.findLastIndex(
            (event) => event[0] === type,
          );
          if (existing !== -1) {
            this.input.splice(existing, 1);
          }
        } else if (type === 5 && this.input.at(-1)?.[0] === 5) {
          this.input[this.input.length - 1] = copy;
          return;
        } else if (type === 19) {
          const existing = this.input.findLastIndex(
            (event) => event[0] === type && event[1] === copy[1],
          );
          if (existing !== -1) {
            this.input.splice(existing, 1);
          }
        }
        if (this.input.length === MAX_INPUT_EVENTS) {
          const discardable = this.input.findIndex(
            (event) =>
              event[0] === 5 ||
              event[0] === 6 ||
              event[0] === 14 ||
              event[0] === 19,
          );
          if (discardable === -1) {
            throw new Error("translated PolkaVM input queue is full");
          }
          this.input.splice(discardable, 1);
        }
        this.input.push(copy);
        return;
      }
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      const type = bytes[0];
      if (
        type === 6 &&
        (Math.abs(view.getInt16(2, true)) > 127 ||
          Math.abs(view.getInt16(4, true)) > 127)
      ) {
        return;
      }
      if (this.imports.includes("pvm_fetch_epoca_inputs")) {
        this.#queueEpocaInput(bytes);
        return;
      }
      if (type === 1 || type === 2) {
        const key = hidToCoreVm(bytes[1]);
        if (key !== undefined) {
          this.#queueCoreInput(key, type === 1 ? 1 : 0);
        }
      } else if (type === 3 || type === 4) {
        const key =
          bytes[1] >= 1 && bytes[1] <= 3 ? 0x9f + bytes[1] : undefined;
        if (key !== undefined) {
          this.#queueCoreInput(key, type === 3 ? 1 : 0);
        }
      } else if (type === 5) {
        const current = [view.getUint16(2, true), view.getUint16(4, true)];
        if (this.pointer) {
          this.#queueCoreInput(
            0xa3,
            Math.max(-128, Math.min(127, current[0] - this.pointer[0])) & 0xff,
          );
          this.#queueCoreInput(
            0xa4,
            Math.max(-128, Math.min(127, current[1] - this.pointer[1])) & 0xff,
          );
        }
        this.pointer = current;
      } else if (type === 6) {
        this.#queueCoreInput(
          0xa3,
          Math.max(-128, Math.min(127, view.getInt16(2, true))) & 0xff,
        );
        this.#queueCoreInput(
          0xa4,
          Math.max(-128, Math.min(127, view.getInt16(4, true))) & 0xff,
        );
      }
    }

    /**
     * Queues one viewport-inset update as the pair the guest ABI defines.
     *
     * The two records carry one axis each, so they are queued together and
     * supersede the queued update of the same type: a guest must never read a
     * new axis beside the previous update's other axis. CoreVM guests have no
     * inset records, so the update is dropped for them.
     */
    sendViewInsets(eventType, left, top, right, bottom) {
      if (this.stopped || this.coreVm) {
        return;
      }
      const horizontal = new Uint8Array(INPUT_EVENT_BYTES);
      horizontal[0] = eventType;
      horizontal[1] = INPUT_INSETS_HORIZONTAL;
      const vertical = new Uint8Array(INPUT_EVENT_BYTES);
      vertical[0] = eventType;
      vertical[1] = INPUT_INSETS_VERTICAL;
      const horizontalView = new DataView(horizontal.buffer);
      horizontalView.setUint16(2, left, true);
      horizontalView.setUint16(4, right, true);
      const verticalView = new DataView(vertical.buffer);
      verticalView.setUint16(2, top, true);
      verticalView.setUint16(4, bottom, true);
      this.input = this.input.filter((record) => record[0] !== eventType);
      while (this.input.length > MAX_INPUT_EVENTS - 2) {
        this.input.shift();
      }
      this.input.push(horizontal, vertical);
    }

    /**
     * How many queued records a guest buffer of `slots` records receives.
     *
     * A buffer that ends between the two halves of an inset update would hand
     * the guest a new axis beside a stale one, so the pair waits for the next
     * poll. A buffer too small to ever hold both is served as-is rather than
     * stalling behind a pair it can never take.
     */
    pollableInputCount(slots) {
      const available = Math.min(slots, this.input.length);
      if (slots < 2 || available === 0) {
        return available;
      }
      const last = this.input[available - 1];
      const startsPair =
        (last[0] === INPUT_SAFE_AREA_INSETS ||
          last[0] === INPUT_KEYBOARD_INSETS) &&
        last[1] === INPUT_INSETS_HORIZONTAL;
      return startsPair ? available - 1 : available;
    }

    setMotionAvailability(availability) {
      if (
        !Number.isInteger(availability) ||
        availability < MOTION_STATUS_UNAVAILABLE ||
        availability > MOTION_STATUS_PERMISSION_DENIED
      ) {
        throw new Error(
          "translated PolkaVM runtime has invalid motion availability",
        );
      }
      this.motionAvailability = availability;
      if (availability !== MOTION_STATUS_AVAILABLE) {
        this.motionSample = null;
      }
    }

    sendMotionSample(bytes) {
      if (this.stopped || !validMotionSample(bytes)) {
        throw new Error(
          "translated PolkaVM runtime received an invalid motion sample",
        );
      }
      this.motionAvailability = MOTION_STATUS_AVAILABLE;
      this.motionSample = bytes.slice();
    }

    setGpuCapabilities(bytes) {
      if (
        this.stopped ||
        !(bytes instanceof Uint8Array) ||
        bytes.byteLength < 56 ||
        bytes.byteLength > 4096
      ) {
        throw new Error("invalid translated WebGPU capabilities");
      }
      this.gpuCapabilities = bytes.slice();
    }

    sendGpuEvent(bytes) {
      if (
        this.stopped ||
        !(bytes instanceof Uint8Array) ||
        bytes.byteLength < 24 ||
        bytes.byteLength > MAX_GPU_EVENT_BYTES ||
        decoder.decode(bytes.subarray(0, 4)) !== "EGE1"
      ) {
        return;
      }
      if (this.gpuEvents.length === MAX_GPU_EVENTS) {
        this.gpuEvents.shift();
      }
      this.gpuEvents.push(bytes.slice());
    }

    sendHostFrameResponse(bytes) {
      if (
        this.stopped ||
        !(bytes instanceof Uint8Array) ||
        !bytes.byteLength ||
        bytes.byteLength > MAX_HOST_FRAME_BYTES
      ) {
        throw new Error("invalid translated host-frame response");
      }
      if (
        this.hostFrameResponses.length === MAX_HOST_FRAMES ||
        this.hostFrameResponseBytes + bytes.byteLength >
          MAX_QUEUED_HOST_FRAME_BYTES
      ) {
        return false;
      }
      this.hostFrameResponses.push(bytes.slice());
      this.hostFrameResponseBytes += bytes.byteLength;
      return true;
    }

    sendMediatedInputResult(handle, status, bytes) {
      const registration = this.mediatedInputRegistrations.get(handle);
      if (
        this.stopped ||
        !registration ||
        registration.status !== MEDIATED_INPUT_STATUS_ACTIVE ||
        this.activeMediatedInputHandle !== handle ||
        !Number.isInteger(status) ||
        ![
          MEDIATED_INPUT_STATUS_READY,
          MEDIATED_INPUT_STATUS_CANCELLED,
          MEDIATED_INPUT_STATUS_PERMISSION_DENIED,
          MEDIATED_INPUT_STATUS_FAILED,
        ].includes(status) ||
        !(bytes instanceof Uint8Array)
      ) {
        throw new Error("invalid translated mediated-input result");
      }
      if (status === MEDIATED_INPUT_STATUS_READY) {
        if (!bytes.byteLength || bytes.byteLength > registration.maxBytes) {
          throw new Error("translated mediated-input result exceeds its registered bound");
        }
        registration.result = bytes.slice();
      } else if (bytes.byteLength) {
        throw new Error("translated mediated-input failure carries unexpected bytes");
      } else {
        registration.result = null;
      }
      this.activeMediatedInputHandle = null;
      registration.status = status;
    }

    #registerMediatedInput(kind, mediaType, maxBytes) {
      if (
        !validMediatedInputToken(kind, MAX_MEDIATED_INPUT_KIND_BYTES) ||
        !validMediatedInputToken(mediaType, MAX_MEDIATED_INPUT_MEDIA_TYPE_BYTES) ||
        !Number.isInteger(maxBytes) ||
        maxBytes < 1 ||
        maxBytes > MAX_MEDIATED_INPUT_BYTES
      ) {
        return -1;
      }
      if (!this.mediatedInputKinds.has(kind)) {
        return -2;
      }
      for (const [handle, registration] of this.mediatedInputRegistrations) {
        if (
          registration.kind === kind &&
          registration.mediaType === mediaType &&
          registration.maxBytes === maxBytes
        ) {
          return handle;
        }
      }
      if (
        this.mediatedInputRegistrations.size === MAX_MEDIATED_INPUT_REGISTRATIONS
      ) {
        return -3;
      }
      do {
        this.nextMediatedInputHandle =
          this.nextMediatedInputHandle >= 0x7fffffff
            ? 1
            : this.nextMediatedInputHandle + 1;
      } while (
        this.mediatedInputRegistrations.has(this.nextMediatedInputHandle)
      );
      this.mediatedInputRegistrations.set(this.nextMediatedInputHandle, {
        kind,
        mediaType,
        maxBytes,
        status: MEDIATED_INPUT_STATUS_REGISTERED,
        result: null,
      });
      return this.nextMediatedInputHandle;
    }

    #triggerMediatedInput(handle) {
      if (this.activeMediatedInputHandle !== null) {
        return 2;
      }
      const registration = this.mediatedInputRegistrations.get(handle);
      if (!registration) {
        return 1;
      }
      registration.status = MEDIATED_INPUT_STATUS_ACTIVE;
      this.activeMediatedInputHandle = handle;
      registration.result = null;
      this.emit({
        type: "mediated-input-request",
        handle,
        kind: registration.kind,
        mediaType: registration.mediaType,
        maxBytes: registration.maxBytes,
      });
      return 0;
    }

    #cancelMediatedInput(handle) {
      const registration = this.mediatedInputRegistrations.get(handle);
      if (!registration) {
        return 1;
      }
      if (registration.status !== MEDIATED_INPUT_STATUS_ACTIVE) {
        return 2;
      }
      registration.status = MEDIATED_INPUT_STATUS_CANCELLED;
      registration.result = null;
      this.activeMediatedInputHandle = null;
      this.emit({ type: "mediated-input-cancel", handle });
      return 0;
    }

    stop() {
      this.stopped = true;
      this.input.length = 0;
      this.coreInput.length = 0;
      this.epocaInput.length = 0;
      this.hostFrameRequests = 0;
      this.hostFrameRequestBytes = 0;
      this.gpuEvents.length = 0;
      this.hostFrameResponses.length = 0;
      this.hostFrameResponseBytes = 0;
      this.mediatedInputRegistrations.clear();
      this.activeMediatedInputHandle = null;
    }

    #resetBudget(hostcalls, gas = this.maxGas) {
      this.hostcalls = hostcalls;
      this.hostcallBytes = MAX_HOSTCALL_BYTES;
      this.tri2dSubmitted = false;
      this.pvm.pvm_set_gas(gas);
    }

    #run(entry, yieldOnFrame, gas = this.maxGas) {
      let status;
      if (this.resumePending) {
        this.resumePending = false;
        status = this.pvm.pvm_resume();
      } else {
        if (entry === undefined) {
          throw new Error("translated PolkaVM entrypoint is missing");
        }
        status = this.pvm.pvm_begin(entry, gas);
      }
      for (;;) {
        if (status === STATUS_FINISHED) {
          if (this.coreVm) {
            throw new Error("CoreVM guest exited");
          }
          return;
        }
        if (status === STATUS_TRAP) {
          throw new Error(
            `translated PolkaVM execution trapped at ${this.pvm.trap_pc.value}`,
          );
        }
        if (status === STATUS_OUT_OF_GAS) {
          throw new Error("translated PolkaVM guest ran out of gas");
        }
        if (status !== STATUS_ECALL) {
          throw new Error(
            `translated PolkaVM returned invalid status ${status}`,
          );
        }
        const importIndex = this.pvm.ecall.value >>> 0;
        const name = this.imports[importIndex];
        if (!name) {
          throw new Error(
            `translated PolkaVM called unknown import ${importIndex}`,
          );
        }
        this.hostcalls--;
        const yielded = this.coreVm
          ? this.#handleCoreVmCall(name)
          : this.#handleCooperativeCall(name);
        if (yielded && yieldOnFrame) {
          this.resumePending = true;
          return;
        }
        if (this.hostcalls === 0) {
          // Resume on the next worker tick after completing this ECALL. Large
          // assets can require more than one bounded hostcall slice, while
          // returning here keeps each slice capped and the worker responsive.
          this.resumePending = true;
          return;
        }
        status = this.pvm.pvm_resume();
      }
    }

    #reg(index) {
      return BigInt.asUintN(64, this.pvm[`r${index}`].value);
    }

    #setReg(index, value) {
      const normalized = this.metadata.is64Bit
        ? BigInt.asIntN(64, BigInt(value))
        : BigInt.asUintN(32, BigInt(value));
      this.pvm[`r${index}`].value = normalized;
    }
    #setU64Result(value) {
      const normalized = BigInt.asUintN(64, BigInt(value));
      this.#setReg(7, normalized);
      if (!this.metadata.is64Bit) {
        this.#setReg(8, normalized >> 32n);
      }
    }

    #u32(value) {
      return Number(value & 0xffffffffn) >>> 0;
    }

    #range(address, length, write = false) {
      address >>>= 0;
      length >>>= 0;
      const end = address + length;
      if (end > 0x100000000) {
        throw new Error(
          "translated PolkaVM guest memory access is out of range",
        );
      }
      const { layout } = this.metadata;
      let physical;
      if (address >= layout.stackLow && end <= layout.stackHigh) {
        physical = layout.stackPhysical + address - layout.stackLow;
      } else if (
        address >= layout.rwAddress &&
        end <=
          layout.rwAddress + this.memory.buffer.byteLength - layout.rwPhysical
      ) {
        physical = layout.rwPhysical + address - layout.rwAddress;
      } else if (
        !write &&
        address >= layout.roAddress &&
        end <= layout.roAddress + layout.roSize
      ) {
        physical = layout.roPhysical + address - layout.roAddress;
      } else {
        throw new Error(
          "translated PolkaVM guest memory access is out of range",
        );
      }
      return new Uint8Array(this.memory.buffer, physical, length);
    }

    #read(address, length) {
      this.#chargeBytes(length);
      return this.#range(address, length).slice();
    }

    #write(address, bytes) {
      this.#chargeBytes(bytes.byteLength);
      this.#range(address, bytes.byteLength, true).set(bytes);
    }

    #chargeBytes(length) {
      if (
        !Number.isSafeInteger(length) ||
        length < 0 ||
        length > this.hostcallBytes
      ) {
        throw new Error(
          "translated PolkaVM guest exceeded hostcall byte budget",
        );
      }
      this.hostcallBytes -= length;
    }

    #readU64(address) {
      const bytes = this.#range(address >>> 0, 8);
      return new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(
        0,
        true,
      );
    }

    #writeU64(address, value) {
      const bytes = this.#range(address >>> 0, 8, true);
      new DataView(bytes.buffer, bytes.byteOffset, 8).setBigUint64(
        0,
        BigInt.asUintN(64, value),
        true,
      );
    }

    #readCString(address) {
      const output = [];
      for (let offset = 0; offset < 255; offset++) {
        let byte;
        try {
          byte = this.#range((address + offset) >>> 0, 1)[0];
        } catch {
          return null;
        }
        if (!byte) {
          return new Uint8Array(output);
        }
        output.push(byte);
      }
      return null;
    }

    // eslint-disable-next-line complexity -- Flat hostcall dispatch mirrors the guest ABI.
    #handleCooperativeCall(name) {
      const a0 = this.#reg(7);
      const a1 = this.#reg(8);
      const a2 = this.#reg(9);
      const a3 = this.#reg(10);
      const a4 = this.#reg(11);
      switch (name) {
        case "host_present_frame": {
          const width = this.#u32(a1);
          const height = this.#u32(a2);
          const stride = this.#u32(a3);
          const rowBytes = width * 4;
          const length = rowBytes * height;
          if (
            !width ||
            !height ||
            stride !== rowBytes ||
            length > MAX_FRAME_BYTES
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          if (this.graphicsProfile !== "framebuffer") {
            this.#setReg(7, 3n);
            return false;
          }
          const source = this.#read(this.#u32(a0), length);
          const pixels = new Uint8Array(length);
          for (let index = 0; index < length; index += 4) {
            pixels[index] = source[index + 2];
            pixels[index + 1] = source[index + 1];
            pixels[index + 2] = source[index];
            pixels[index + 3] = source[index + 3];
          }
          this.emit({ type: "frame", width, height, pixels }, [pixels.buffer]);
          this.#setReg(7, 0n);
          return false;
        }
        case "host_tri2d_submit": {
          const length = this.#u32(a1);
          if (!length || length > MAX_TRI2D_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          if (this.graphicsProfile !== "tri2d") {
            this.#setReg(7, 3n);
            return false;
          }
          if (this.tri2dSubmitted) {
            this.#setReg(7, 2n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          this.emit({ type: "tri2d", bytes }, [bytes.buffer]);
          this.tri2dSubmitted = true;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_ui_semantics_submit": {
          const length = this.#u32(a1);
          if (!length || length > MAX_UI_SEMANTICS_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          if (this.uiSemanticsSubmitted) {
            this.#setReg(7, 2n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          if (!validUiSemantics(bytes)) {
            this.#setReg(7, 1n);
            return false;
          }
          this.emit({ type: "ui-semantics", bytes }, [bytes.buffer]);
          this.uiSemanticsSubmitted = true;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_ui_output_submit": {
          const length = this.#u32(a1);
          if (length < UI_OUTPUT_HEADER_BYTES || length > MAX_UI_OUTPUT_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          this.#chargeBytes(length);
          if (this.uiOutputSubmitted) {
            this.#setReg(7, 2n);
            return false;
          }
          let bytes;
          try {
            bytes = this.#range(this.#u32(a0), length).slice();
          } catch {
            this.#setReg(7, 1n);
            return false;
          }
          const output = decodeUiOutput(bytes);
          if (output === null) {
            this.#setReg(7, 1n);
            return false;
          }
          this.emit({ type: "ui-output", output });
          this.uiOutputSubmitted = true;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_gpu_capabilities": {
          if (!isWebGpuProfile(this.graphicsProfile)) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          if (this.gpuCapabilities === null) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          const capacity = this.#u32(a1);
          const required = this.gpuCapabilities.byteLength;
          if (capacity < required) {
            this.#setReg(7, BigInt(-required));
            return false;
          }
          this.#write(this.#u32(a0), this.gpuCapabilities);
          this.#setReg(7, BigInt(required));
          return false;
        }
        case "host_gpu_submit": {
          if (!isWebGpuProfile(this.graphicsProfile)) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          const length = this.#u32(a1);
          if (this.gpuCapabilities === null) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          if (
            !length ||
            length > MAX_GPU_BATCH_BYTES ||
            this.gpuSubmits === MAX_GPU_SUBMITS_PER_UPDATE
          ) {
            this.#setReg(
              7,
              BigInt(
                this.gpuSubmits === MAX_GPU_SUBMITS_PER_UPDATE
                  ? GPU_ERROR_QUOTA_EXCEEDED
                  : GPU_ERROR_MALFORMED_BATCH,
              ),
            );
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          const sequence = this.#gpuBatchSequence(bytes);
          if (sequence === null) {
            this.#setReg(7, BigInt(GPU_ERROR_MALFORMED_BATCH));
            return false;
          }
          if (
            this.graphicsProfile !== "webgpu" &&
            this.#gpuBatchUsesCompute(bytes)
          ) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          if (sequence <= this.gpuLastSequence) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          this.gpuSubmits++;
          this.gpuLastSequence = sequence;
          this.emit({ type: "gpu-batch", bytes }, [bytes.buffer]);
          this.#setReg(7, 0n);
          return false;
        }
        case "host_gpu_receive": {
          if (!isWebGpuProfile(this.graphicsProfile)) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          const event = this.gpuEvents[0];
          if (event === undefined) {
            this.#setReg(7, 0n);
            return false;
          }
          const capacity = this.#u32(a1);
          if (capacity < event.byteLength) {
            this.#setReg(7, BigInt(-event.byteLength));
            return false;
          }
          this.#write(this.#u32(a0), event);
          this.gpuEvents.shift();
          this.#setReg(7, BigInt(event.byteLength));
          return false;
        }
        case "host_poll_input": {
          const capacity = this.#u32(a1);
          const slots = Math.floor(capacity / INPUT_EVENT_BYTES);
          const count = this.pollableInputCount(slots);
          const output = new Uint8Array(count * INPUT_EVENT_BYTES);
          for (let index = 0; index < count; index++) {
            output.set(this.input.shift(), index * INPUT_EVENT_BYTES);
          }
          this.#write(this.#u32(a0), output);
          this.#setReg(7, BigInt(output.byteLength));
          return false;
        }
        case "host_motion_read":
          return this.#readMotion();
        case POINTER_CAPTURE_IMPORT: {
          const status = this.#requestPointerCapture(this.#u32(a0));
          this.#setReg(7, BigInt(status));
          return false;
        }
        case "host_input_register": {
          const kindLength = this.#u32(a1);
          const mediaTypeLength = this.#u32(a3);
          let result = -1;
          if (
            kindLength > 0 &&
            kindLength <= MAX_MEDIATED_INPUT_KIND_BYTES &&
            mediaTypeLength > 0 &&
            mediaTypeLength <= MAX_MEDIATED_INPUT_MEDIA_TYPE_BYTES
          ) {
            this.#chargeBytes(kindLength + mediaTypeLength);
            try {
              const kind = strictDecoder.decode(
                this.#read(this.#u32(a0), kindLength),
              );
              const mediaType = strictDecoder.decode(
                this.#read(this.#u32(a2), mediaTypeLength),
              );
              result = this.#registerMediatedInput(
                kind,
                mediaType,
                this.#u32(a4),
              );
            } catch {
              result = -1;
            }
          }
          this.#setReg(7, BigInt(result));
          return false;
        }
        case "host_input_trigger": {
          this.#setReg(
            7,
            BigInt(this.#triggerMediatedInput(this.#u32(a0))),
          );
          return false;
        }
        case "host_input_status": {
          const registration = this.mediatedInputRegistrations.get(
            this.#u32(a0),
          );
          this.#setReg(7, BigInt(registration?.status ?? 0));
          return false;
        }
        case "host_input_read": {
          const registration = this.mediatedInputRegistrations.get(
            this.#u32(a0),
          );
          if (
            !registration ||
            registration.status !== MEDIATED_INPUT_STATUS_READY ||
            !(registration.result instanceof Uint8Array)
          ) {
            this.#setReg(7, 0n);
            return false;
          }
          const required = registration.result.byteLength;
          if (this.#u32(a2) < required) {
            this.#setReg(7, BigInt(-required));
            return false;
          }
          this.#chargeBytes(required);
          this.#write(this.#u32(a1), registration.result);
          registration.result = null;
          registration.status = MEDIATED_INPUT_STATUS_REGISTERED;
          this.#setReg(7, BigInt(required));
          return false;
        }
        case "host_input_cancel": {
          this.#setReg(
            7,
            BigInt(this.#cancelMediatedInput(this.#u32(a0))),
          );
          return false;
        }
        case UPDATE_AFTER_IMPORT: {
          const delayMs = this.#u32(a0);
          this.updateAfterMs =
            this.updateAfterMs === null
              ? delayMs
              : Math.min(this.updateAfterMs, delayMs);
          return false;
        }
        case "polkadot_host_0_1_core_clock_monotonic": {
          this.#chargeBytes(8);
          const timeMs =
            this.timeMs ?? performance.now() - this.clockStartedAt;
          this.#writeU64(
            this.#u32(a0),
            BigInt(Math.max(0, Math.trunc(timeMs * 1_000_000))),
          );
          this.#setReg(7, 0n);
          return false;
        }
        case "polkadot_host_0_1_core_clock_wall":
          this.#chargeBytes(8);
          this.#writeU64(this.#u32(a0), BigInt(Date.now()) * 1_000_000n);
          this.#setReg(7, 0n);
          return false;
        case "polkadot_host_0_1_core_random": {
          const length = this.#u32(a1);
          if (length === 0) {
            this.#setReg(7, BigInt(CORE_STATUS_INVALID));
            return false;
          }
          if (length > MAX_CORE_RANDOM_BYTES) {
            this.#setReg(7, BigInt(CORE_STATUS_LIMIT));
            return false;
          }
          this.#chargeBytes(length);
          let bytes;
          try {
            const browserCrypto = globalThis.crypto;
            if (typeof browserCrypto?.getRandomValues !== "function") {
              this.#setReg(7, BigInt(CORE_STATUS_DENIED));
              return false;
            }
            bytes = new Uint8Array(length);
            browserCrypto.getRandomValues(bytes);
          } catch {
            this.#setReg(7, BigInt(CORE_STATUS_DENIED));
            return false;
          }
          this.#range(this.#u32(a0), length, true).set(bytes);
          this.#setReg(7, 0n);
          return false;
        }
        case "host_time_ms": {
          const timeMs = this.timeMs ?? performance.now() - this.clockStartedAt;
          this.#setU64Result(BigInt(Math.max(0, Math.trunc(timeMs))));
          return false;
        }
        case "host_random_fill": {
          const length = this.#u32(a1);
          const end = this.randomOffset + length;
          if (
            !length ||
            length > MAX_RANDOM_BYTES_PER_CALL ||
            end > this.randomBytes.byteLength
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          this.#chargeBytes(length);
          this.#write(
            this.#u32(a0),
            this.randomBytes.subarray(this.randomOffset, end),
          );
          this.randomOffset = end;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_sleep_ms":
          this.timeMs =
            (this.timeMs ?? performance.now() - this.clockStartedAt) +
            Math.min(this.#u32(a0), 50);
          return false;
        case "host_audio_submit": {
          if (!this.audioEnabled) {
            this.#setReg(7, 3n);
            return false;
          }
          const sampleCount = this.#u32(a1);
          if (
            !sampleCount ||
            sampleCount % 2 ||
            sampleCount > MAX_AUDIO_SAMPLES
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          const samples = this.#read(this.#u32(a0), sampleCount * 2);
          this.emit(
            { type: "audio", sampleRate: 48000, channels: 2, samples },
            [samples.buffer],
          );
          this.#setReg(7, 0n);
          return false;
        }
        case "host_frame_send": {
          const length = this.#u32(a1);
          if (!length || length > MAX_HOST_FRAME_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          if (
            this.hostFrameRequests === MAX_HOST_FRAMES ||
            this.hostFrameRequestBytes + length > MAX_QUEUED_HOST_FRAME_BYTES
          ) {
            this.#setReg(7, 2n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          this.emit({ type: "host-frame-request", bytes }, [bytes.buffer]);
          this.hostFrameRequests++;
          this.hostFrameRequestBytes += length;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_frame_poll": {
          const response = this.hostFrameResponses[0];
          if (response === undefined) {
            this.#setReg(7, 0n);
            return false;
          }
          const capacity = this.#u32(a1);
          if (capacity < response.byteLength) {
            this.#setReg(7, BigInt(-response.byteLength));
            return false;
          }
          this.#write(this.#u32(a0), response);
          this.hostFrameResponses.shift();
          this.hostFrameResponseBytes -= response.byteLength;
          this.#setReg(7, BigInt(response.byteLength));
          return false;
        }
        case "host_asset_read": {
          const nameLength = this.#u32(a1);
          const offset = this.#u32(a2);
          const destination = this.#u32(a3);
          const capacity = this.#u32(a4);
          if (!nameLength || nameLength > 1024) {
            this.#setReg(7, 0n);
            return false;
          }
          const assetName = decoder.decode(
            this.#read(this.#u32(a0), nameLength),
          );
          const asset = this.assets.get(assetName);
          if (!asset || offset >= asset.byteLength) {
            this.#setReg(7, 0n);
            return false;
          }
          const length = Math.min(
            capacity,
            asset.byteLength - offset,
            16 * 1024 * 1024,
          );
          this.#write(destination, asset.subarray(offset, offset + length));
          this.#setReg(7, BigInt(length));
          return false;
        }
        case "host_save_submit": {
          const length = this.#u32(a1);
          if (!length || length > MAX_SAVE_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          this.emit({ type: "save", bytes }, [bytes.buffer]);
          this.#setReg(7, 0n);
          return false;
        }
        case "host_log": {
          const length = Math.min(this.#u32(a1), MAX_LOG_BYTES);
          const message = decoder.decode(this.#read(this.#u32(a0), length));
          this.emit({ type: "log", message });
          return false;
        }
        default:
          throw new Error(
            `translated PolkaVM guest uses unsupported import ${name}`,
          );
      }
    }

    #gpuBatchSequence(bytes) {
      if (
        bytes.byteLength < 24 ||
        decoder.decode(bytes.subarray(0, 4)) !== "EPG1"
      ) {
        return null;
      }
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      if (
        view.getUint16(4, true) !== 1 ||
        view.getUint16(6, true) !== 0 ||
        view.getUint32(8, true) !== bytes.byteLength
      ) {
        return null;
      }
      const commandCount = view.getUint32(12, true);
      const sequence = view.getBigUint64(16, true);
      if (sequence === 0n || commandCount > MAX_GPU_COMMANDS) {
        return null;
      }
      let offset = 24;
      for (let index = 0; index < commandCount; index++) {
        if (
          offset + 8 > bytes.byteLength ||
          view.getUint16(offset + 2, true) !== 0
        ) {
          return null;
        }
        const commandBytes = view.getUint32(offset + 4, true);
        if (
          commandBytes < 8 ||
          commandBytes % 4 ||
          commandBytes > bytes.byteLength - offset
        ) {
          return null;
        }
        offset += commandBytes;
      }
      return offset === bytes.byteLength ? sequence : null;
    }

    #gpuBatchUsesCompute(bytes) {
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      const commandCount = view.getUint32(12, true);
      let offset = 24;
      for (let index = 0; index < commandCount; index++) {
        if (isComputeOpcode(view.getUint16(offset, true))) {
          return true;
        }
        offset += view.getUint32(offset + 4, true);
      }
      return false;
    }

    #setupCoreVm() {
      let sp = BigInt(this.metadata.layout.stackHigh);
      const argc = 1n;
      sp -= (1n + argc + 1n + 0n + 1n + 4n) * 8n;
      const addressInit = sp;
      let pointer = sp;
      this.#writeU64(Number(pointer), argc);
      pointer += 8n;
      const argument = encoder.encode("./quake\0");
      sp -= BigInt(argument.byteLength);
      this.#write(Number(sp), argument);
      this.#writeU64(Number(pointer), sp);
      pointer += 16n;
      pointer += 8n;
      this.#writeU64(Number(pointer), 6n);
      this.#writeU64(Number(pointer + 8n), 4096n);
      this.#setReg(1, sp);
      this.#setReg(7, addressInit);
    }

    #readMotion() {
      if (this.motionAvailability === MOTION_STATUS_UNAVAILABLE) {
        this.#setReg(7, BigInt(MOTION_ERROR_UNAVAILABLE));
        return false;
      }
      if (this.motionAvailability === MOTION_STATUS_PERMISSION_DENIED) {
        this.#setReg(7, BigInt(MOTION_ERROR_PERMISSION_DENIED));
        return false;
      }
      if (this.motionSample === null) {
        this.#setReg(7, 0n);
        return false;
      }
      if (this.#u32(this.#reg(8)) < MOTION_SAMPLE_BYTES) {
        this.#setReg(7, BigInt(MOTION_ERROR_BUFFER_TOO_SMALL));
        return false;
      }
      try {
        this.#write(this.#u32(this.#reg(7)), this.motionSample);
      } catch {
        this.#setReg(7, BigInt(MOTION_ERROR_INVALID_GUEST_RANGE));
        return false;
      }
      this.motionSample = null;
      this.#setReg(7, BigInt(MOTION_SAMPLE_BYTES));
      return false;
    }

    #queueEpocaInput(bytes) {
      const event = bytes.slice();
      if (event[0] === 5 || event[0] === 6) {
        const existing = this.epocaInput.findIndex(
          (queued) => queued[0] === event[0],
        );
        if (existing !== -1) {
          this.epocaInput[existing] = event;
          return;
        }
      }
      if (this.epocaInput.length === 256) {
        this.epocaInput.shift();
      }
      this.epocaInput.push(event);
    }

    #queueCoreInput(key, value) {
      if (!value && (key === 0xa3 || key === 0xa4)) {
        return;
      }
      if (key === 0xa3 || key === 0xa4) {
        const existing = this.coreInput.find((event) => event[0] === key);
        if (existing) {
          existing[1] = value;
          return;
        }
      }
      if (this.coreInput.length === 256) {
        this.coreInput.shift();
      }
      this.coreInput.push([key, value]);
    }

    // eslint-disable-next-line complexity -- Flat hostcall dispatch mirrors the guest ABI.
    #handleCoreVmCall(name) {
      switch (name) {
        case "host_frame_send":
        case "host_frame_poll":
        case "host_motion_read":
        case POINTER_CAPTURE_IMPORT:
        case "polkadot_host_0_1_core_clock_monotonic":
        case "polkadot_host_0_1_core_clock_wall":
        case "polkadot_host_0_1_core_random":
        case UPDATE_AFTER_IMPORT:
          return this.#handleCooperativeCall(name);
        case "pvm_set_palette": {
          const palette = this.#read(this.#u32(this.#reg(7)), 256 * 3);
          for (let index = 0; index < 256; index++) {
            const offset = index * 3;
            this.palette[index] =
              palette[offset] |
              (palette[offset + 1] << 8) |
              (palette[offset + 2] << 16) |
              0xff000000;
          }
          return false;
        }
        case "pvm_display": {
          const width = this.#u32(this.#reg(7));
          const height = this.#u32(this.#reg(8));
          const length = width * height;
          if (!width || !height || length > MAX_FRAME_BYTES / 4) {
            throw new Error("CoreVM guest supplied invalid frame dimensions");
          }
          const indices = this.#read(this.#u32(this.#reg(9)), length);
          const pixels = new Uint8Array(length * 4);
          const rgba = new Uint32Array(pixels.buffer);
          for (let index = 0; index < length; index++) {
            rgba[index] = this.palette[indices[index]];
          }
          this.emit({ type: "frame", width, height, pixels }, [pixels.buffer]);
          return true;
        }
        case "pvm_fetch_epoca_inputs": {
          const count = Math.min(
            this.#u32(this.#reg(8)),
            this.epocaInput.length,
          );
          const output = new Uint8Array(count * INPUT_EVENT_BYTES);
          for (let index = 0; index < count; index++) {
            output.set(this.epocaInput.shift(), index * INPUT_EVENT_BYTES);
          }
          this.#write(this.#u32(this.#reg(7)), output);
          this.#setReg(7, BigInt(count));
          return false;
        }
        case "pvm_fetch_inputs": {
          const count = Math.min(
            this.#u32(this.#reg(8)),
            this.coreInput.length,
          );
          const output = new Uint8Array(count * 2);
          for (let index = 0; index < count; index++) {
            output.set(this.coreInput.shift(), index * 2);
          }
          this.#write(this.#u32(this.#reg(7)), output);
          this.#setReg(7, BigInt(count));
          return false;
        }
        case "pvm_asset_read": {
          const nameLength = this.#u32(this.#reg(8));
          const offset = this.#u32(this.#reg(9));
          const destination = this.#u32(this.#reg(10));
          const capacity = this.#u32(this.#reg(11));
          if (!nameLength || nameLength > 1024) {
            this.#setReg(7, 0n);
            return false;
          }
          const assetName = decoder.decode(
            this.#read(this.#u32(this.#reg(7)), nameLength),
          );
          const asset = this.assets.get(assetName);
          if (!asset || offset >= asset.byteLength) {
            this.#setReg(7, 0n);
            return false;
          }
          const length = Math.min(
            capacity,
            asset.byteLength - offset,
            16 * 1024 * 1024,
          );
          this.#write(destination, asset.subarray(offset, offset + length));
          this.#setReg(7, BigInt(length));
          return false;
        }
        case "host_audio_submit": {
          if (!this.audioEnabled) {
            this.#setReg(7, 3n);
            return false;
          }
          const sampleCount = this.#u32(this.#reg(8));
          if (
            !sampleCount ||
            sampleCount % 2 ||
            sampleCount > MAX_AUDIO_SAMPLES
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          const samples = this.#read(this.#u32(this.#reg(7)), sampleCount * 2);
          this.emit(
            { type: "audio", sampleRate: 48000, channels: 2, samples },
            [samples.buffer],
          );
          this.#setReg(7, 0n);
          return false;
        }
        case "pvm_time_ms": {
          const timeMs = this.timeMs ?? performance.now() - this.clockStartedAt;
          this.#setU64Result(BigInt(Math.max(0, Math.trunc(timeMs))));
          return false;
        }
        case "host_log": {
          const length = Math.min(this.#u32(this.#reg(8)), MAX_LOG_BYTES);
          const message = decoder.decode(
            this.#read(this.#u32(this.#reg(7)), length),
          );
          this.emit({ type: "log", message });
          return false;
        }
        case "pvm_yield":
          return true;
        case "pvm_init_audio": {
          const channels = this.#u32(this.#reg(7));
          const bitsPerSample = this.#u32(this.#reg(8));
          const sampleRate = this.#u32(this.#reg(9));
          if (
            bitsPerSample !== 16 ||
            channels < 1 ||
            channels > 2 ||
            sampleRate < 8000 ||
            sampleRate > 96000
          ) {
            this.#setReg(7, 0n);
          } else {
            this.audioChannels = channels;
            this.audioSampleRate = sampleRate;
            this.#setReg(7, 1n);
          }
          return false;
        }
        case "pvm_output_audio": {
          const frames = this.#u32(this.#reg(8));
          const sampleCount = Math.min(frames * this.audioChannels, 1024 * 64);
          if (this.audioChannels && sampleCount) {
            const samples = this.#read(
              this.#u32(this.#reg(7)),
              sampleCount * 2,
            );
            this.emit(
              {
                type: "audio",
                sampleRate: this.audioSampleRate,
                channels: this.audioChannels,
                samples,
              },
              [samples.buffer],
            );
          }
          return false;
        }
        case "pvm_syscall":
          return this.#handleCoreVmSyscall();
        default:
          throw new Error(
            `translated CoreVM guest uses unsupported import ${name}`,
          );
      }
    }

    #handleCoreVmSyscall() {
      const syscall = this.#reg(7);
      const a1 = this.#reg(8);
      const a2 = this.#reg(9);
      const a3 = this.#reg(10);
      if (syscall === SYS_READ) {
        this.#setReg(7, this.#readFile(a1, a2, a3));
      } else if (syscall === SYS_READV || syscall === SYS_WRITEV) {
        if (!a3 || a3 > IOV_MAX) {
          this.#setReg(7, errno(EINVAL));
          return false;
        }
        let total = 0n;
        for (let index = 0n; index < a3; index++) {
          let address;
          let length;
          try {
            address = this.#readU64(this.#u32(a2 + index * 16n));
            length = this.#readU64(this.#u32(a2 + index * 16n + 8n));
          } catch {
            this.#setReg(7, errno(EFAULT));
            return false;
          }
          const result =
            syscall === SYS_READV
              ? this.#readFile(a1, address, length)
              : this.#writeFile(a1, address, length);
          if (BigInt.asIntN(64, result) < 0n) {
            this.#setReg(7, result);
            return false;
          }
          total += length;
        }
        this.#setReg(7, total);
      } else if (syscall === SYS_EXIT) {
        if (a1 === 0n) {
          throw new Error("CoreVM guest exited");
        }
        throw new Error(`CoreVM guest exited with status ${a1}`);
      } else if (syscall === SYS_OPENAT) {
        if (a1 !== AT_FDCWD) {
          this.#setReg(7, errno(ENOSYS));
          return false;
        }
        const pathBytes = this.#readCString(this.#u32(a2));
        if (!pathBytes) {
          this.#setReg(7, errno(EFAULT));
          return false;
        }
        const path = normalizedPath(decoder.decode(pathBytes));
        const flags = a3;
        const asset = this.assets.get(path);
        if (!asset) {
          this.#setReg(7, errno(ENOENT));
        } else if (flags & 3n) {
          this.#setReg(7, errno(EACCES));
        } else {
          const fd = this.nextFd++;
          this.fds.set(fd, { bytes: asset, position: 0n });
          this.#setReg(7, BigInt(fd));
        }
      } else if (syscall === SYS_LSEEK) {
        this.#setReg(7, this.#seekFile(a1, a2, a3));
      } else if (syscall === SYS_CLOSE) {
        const fd = this.#fdNumber(a1);
        if (fd === null || !this.fds.delete(fd)) {
          this.#setReg(7, errno(EBADF));
        } else {
          this.#setReg(7, 0n);
        }
      } else {
        this.#setReg(7, errno(ENOSYS));
      }
      return false;
    }

    #fdNumber(value) {
      return value <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(value) : null;
    }

    #readFile(fdValue, address, length) {
      const fd = this.#fdNumber(fdValue);
      const file = fd === null ? null : this.fds.get(fd);
      if (!file) {
        return errno(EBADF);
      }
      if (
        address + length > 0x100000000n ||
        length > BigInt(MAX_HOSTCALL_BYTES)
      ) {
        return errno(EFAULT);
      }
      const end =
        file.position + length < BigInt(file.bytes.byteLength)
          ? file.position + length
          : BigInt(file.bytes.byteLength);
      if (file.position >= end) {
        return 0n;
      }
      const count = Number(end - file.position);
      try {
        this.#write(
          this.#u32(address),
          file.bytes.subarray(
            Number(file.position),
            Number(file.position) + count,
          ),
        );
      } catch {
        return errno(EFAULT);
      }
      file.position += BigInt(count);
      return BigInt(count);
    }

    #writeFile(fdValue, address, length) {
      if (fdValue !== 1n && fdValue !== 2n) {
        return errno(EBADF);
      }
      if (
        address + length > 0x100000000n ||
        length > BigInt(MAX_HOSTCALL_BYTES)
      ) {
        return errno(EFAULT);
      }
      try {
        const byteLength = Number(length);
        this.#chargeBytes(byteLength);
        const bytes = this.#range(this.#u32(address), byteLength);
        const message = decoder.decode(bytes.subarray(0, MAX_LOG_BYTES));
        if (message) {
          this.emit({ type: "log", message });
        }
      } catch {
        return errno(EFAULT);
      }
      return length;
    }

    #seekFile(fdValue, offsetValue, whence) {
      const fd = this.#fdNumber(fdValue);
      const file = fd === null ? null : this.fds.get(fd);
      if (!file) {
        return errno(EBADF);
      }
      const offset = BigInt.asIntN(64, offsetValue);
      const fileLength = BigInt(file.bytes.byteLength);
      if (whence === 0n) {
        file.position = BigInt.asUintN(64, offset);
      } else if (whence === 1n) {
        file.position = BigInt.asUintN(
          64,
          BigInt.asIntN(64, file.position) + offset,
        );
        if (file.position > fileLength) {
          file.position = fileLength;
        }
      } else if (whence === 2n) {
        file.position = BigInt.asUintN(
          64,
          BigInt.asIntN(64, fileLength) + offset,
        );
        if (file.position > fileLength) {
          file.position = fileLength;
        }
      } else {
        return errno(EINVAL);
      }
      return file.position;
    }
  }

  globalThis.decodePolkaVmUiOutput = decodeUiOutput;
  globalThis.TranslatedPolkaVmRuntime = TranslatedPolkaVmRuntime;
})();

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/*
 * Browser implementation of the experimental `polkadot-host-computer/0.1`
 * contract over translated PolkaVM guests.
 *
 * The native reference implementation lives in
 * rust/crates/polkavm-host-runtime/src/computer.rs; this file mirrors its observable
 * semantics (status codes, limits, record encodings, supervisor state
 * machine) and is held to it by running the same conformance fixtures in
 * test/computer.test.mjs. Guests are not recompiled for the web: their
 * PolkaVM bytecode is translated to wasm by the staged translator in
 * polkavm-browser-runtime.wasm, and every hostcall is handled here.
 *
 * Networking is denied unless the embedding Host injects a byte-stream provider.
 * Workspace management is denied unless the embedder grants host.workspace
 * to the root computer through setWorkspaceEnabled.
 */

(() => {
  const STATUS_FINISHED = -1;
  const STATUS_ECALL = -2;
  const STATUS_TRAP = -3;
  const STATUS_OUT_OF_GAS = -4;

  // Status codes and limits mirrored from computer.rs.
  const STATUS_WOULD_BLOCK = -1;
  const STATUS_BAD_HANDLE = -2;
  const STATUS_INVALID = -3;
  const STATUS_NOT_FOUND = -4;
  const STATUS_DENIED = -5;
  const STATUS_LIMIT = -6;
  const STATUS_EXISTS = -7;
  const STATUS_NOT_DIRECTORY = -8;
  const STATUS_IS_DIRECTORY = -9;
  const STATUS_NOT_EMPTY = -10;

  const COMPUTER_TTY_HANDLE = 1;
  const TTY_MODE_RAW = 1;
  const TTY_MODE_ECHO = 2;
  const MAX_TTY_INPUT_BYTES = 64 * 1024;
  const MAX_TTY_OUTPUT_BYTES = 1024 * 1024;
  const MAX_COMPUTER_FILES = 64;
  const MAX_COMPUTER_DIRECTORIES = 256;
  const MAX_COMPUTER_FILE_BYTES = 1024 * 1024;
  const MAX_OPEN_COMPUTER_FILES = 16;
  const MAX_COMPUTER_PATH_BYTES = 200;
  const MAX_COMPUTER_CONTEXT_BYTES = 64 * 1024;
  const MAX_COMPUTER_CONTEXT_ENTRIES = 1024;
  const MAX_COMPUTER_PROCESSES = 4;
  const MAX_BACKGROUND_PROCESSES = 4;
  const MAX_WORKSPACE_CHILDREN = 9;
  const FIRST_FILE_HANDLE = 16;
  const MAX_TTY_TRANSFER = 64 * 1024;
  const MAX_RANDOM_BYTES = 4 * 1024;
  const MAX_OPEN_SOCKETS = 4;
  const FIRST_SOCKET_HANDLE = 0x1000;
  const MAX_NET_ADDRESS_BYTES = 256;
  const MAX_NET_BUFFER_BYTES = 1024 * 1024;
  const MAX_NET_BUFFER_CHUNKS = 1024;
  const MAX_FS_TRANSFER = 1024 * 1024;
  const FS_OPEN_READ = 1;
  const FS_OPEN_WRITE = 2;
  const FS_OPEN_CREATE = 4;
  const FS_OPEN_TRUNCATE = 8;
  const FS_OPEN_EXCLUSIVE = 16;
  const FS_OPEN_APPEND = 32;
  const FAULTED_CHILD_STATUS = 139;
  const MAX_FAULT_POPS_PER_RUN = 32;
  const MAX_DRIVE_STEPS = 1024;
  const MAX_WORKSPACE_DRIVE_STEPS = 64;
  const AT_PAGESZ = 6n;

  const decoder = new TextDecoder("utf-8", { fatal: true });
  const encoder = new TextEncoder();

  function readMetadata(module) {
    const sections = WebAssembly.Module.customSections(
      module,
      "epoca.pvm.meta",
    );
    if (sections.length !== 1) {
      throw new Error("translated PolkaVM module has invalid metadata");
    }
    const bytes = new Uint8Array(sections[0]);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let offset = 0;
    const requireBytes = (length) => {
      if (offset + length > bytes.byteLength) {
        throw new Error("translated PolkaVM metadata is truncated");
      }
    };
    const readU16 = () => {
      requireBytes(2);
      const value = view.getUint16(offset, true);
      offset += 2;
      return value;
    };
    const readU32 = () => {
      requireBytes(4);
      const value = view.getUint32(offset, true);
      offset += 4;
      return value;
    };
    const readString = (length) => {
      requireBytes(length);
      const value = new TextDecoder().decode(
        bytes.subarray(offset, offset + length),
      );
      offset += length;
      return value;
    };
    requireBytes(4);
    if (new TextDecoder().decode(bytes.subarray(0, 4)) !== "EPM2") {
      throw new Error("translated PolkaVM metadata has an incompatible ABI");
    }
    offset = 4;
    const is64Bit = readU32() !== 0;
    const names = [
      "roAddress",
      "roSize",
      "roPhysical",
      "rwAddress",
      "rwSize",
      "rwPhysical",
      "heapBase",
      "heapLimit",
      "stackLow",
      "stackHigh",
      "stackPhysical",
    ];
    const layout = {};
    for (const name of names) {
      layout[name] = readU32();
    }
    const imports = [];
    const importCount = readU32();
    for (let index = 0; index < importCount; index++) {
      const length = readU16();
      imports.push(length ? readString(length) : null);
    }
    const exports = new Map();
    const exportCount = readU32();
    for (let index = 0; index < exportCount; index++) {
      const name = readString(readU16());
      const block = readU32();
      exports.set(name, block);
    }
    return { is64Bit, layout, imports, exports };
  }

  function validPath(path) {
    if (
      typeof path !== "string" ||
      encoder.encode(path).byteLength > MAX_COMPUTER_PATH_BYTES ||
      decoder.decode(encoder.encode(path)) !== path ||
      (path !== "/home" && !path.startsWith("/home/")) ||
      path.endsWith("/") ||
      path.includes("//") ||
      path.includes("\0") ||
      path.split("/").some((segment) => segment === "." || segment === "..")
    ) {
      return null;
    }
    return path;
  }

  function parentPath(path) {
    return path.slice(0, path.lastIndexOf("/"));
  }

  function comparePaths(left, right) {
    const a = encoder.encode(left);
    const b = encoder.encode(right);
    for (let index = 0; index < Math.min(a.length, b.length); index++) {
      if (a[index] !== b[index]) return a[index] - b[index];
    }
    return a.length - b.length;
  }

  /** One in-memory namespace shared by every process in a supervisor tree.
   * fs_sync promises visibility only, not durable persistence. */
  class ComputerFilesystem {
    constructor() {
      this.files = new Map();
      this.entries = new Map([["/home", { kind: 2, mtimeNs: 0n, inode: 1n }]]);
      this.nextInode = 2n;
      this.clockNs = 0n;
      this.modified = new Set();
      this.removed = new Set();
      this.metadataDirty = false;
      this.openPaths = new Map();
      this.clients = new Set();
    }

    canMutate(inodes = 0) {
      return (
        this.clockNs < 0xffffffffffffffffn &&
        this.nextInode + BigInt(inodes) <= 0xffffffffffffffffn
      );
    }

    tick() {
      const now = BigInt(Date.now()) * 1000000n;
      this.clockNs = now > this.clockNs ? now : this.clockNs + 1n;
      return this.clockNs;
    }

    touch(path, timestamp = this.tick()) {
      this.entries.get(path).mtimeNs = timestamp;
      this.metadataDirty = true;
    }

    create(path, kind, dirty = true, timestamp = this.tick()) {
      this.entries.set(path, {
        kind,
        mtimeNs: timestamp,
        inode: this.nextInode++,
      });
      if (dirty) {
        this.touch(parentPath(path), timestamp);
        this.metadataDirty = true;
      }
    }

    missingStatus(path) {
      for (
        let ancestor = parentPath(path);
        ancestor.startsWith("/home");
        ancestor = parentPath(ancestor)
      ) {
        if (this.entries.get(ancestor)?.kind === 1) return STATUS_NOT_DIRECTORY;
      }
      return STATUS_NOT_FOUND;
    }

    parentStatus(path) {
      const parent = this.entries.get(parentPath(path));
      return parent === undefined
        ? this.missingStatus(parentPath(path))
        : parent.kind !== 2
          ? STATUS_NOT_DIRECTORY
          : 0;
    }

    directoryCount() {
      return this.entries.size - this.files.size - 1;
    }

    isOpen(path, subtree = false) {
      for (const open of this.openPaths.keys()) {
        if (open === path || (subtree && open.startsWith(`${path}/`)))
          return true;
      }
      return false;
    }

    mountFile(path, bytes) {
      if (validPath(path) === null)
        throw new Error(`invalid computer file path ${path}`);
      if (bytes.byteLength > MAX_COMPUTER_FILE_BYTES)
        throw new Error("mounted file exceeds the size limit");
      if (this.entries.get(path)?.kind === 2 || this.isOpen(path))
        throw new Error("mounted path is a directory or open");
      if (!this.files.has(path) && this.files.size >= MAX_COMPUTER_FILES)
        throw new Error("computer filesystem file limit exceeded");
      const parents = [];
      for (
        let parent = parentPath(path);
        parent !== "/home";
        parent = parentPath(parent)
      ) {
        const entry = this.entries.get(parent);
        if (entry?.kind === 1)
          throw new Error("mounted file parent is not a directory");
        if (entry === undefined) parents.push(parent);
      }
      if (this.directoryCount() + parents.length > MAX_COMPUTER_DIRECTORIES)
        throw new Error("computer filesystem directory limit exceeded");
      const creations = parents.length + (this.entries.has(path) ? 0 : 1);
      if (creations > 0 && !this.canMutate(creations))
        throw new Error("computer filesystem metadata limit exceeded");
      const copy = Uint8Array.from(bytes);
      const timestamp = creations > 0 ? this.tick() : this.clockNs;
      for (const parent of parents.reverse())
        this.create(parent, 2, false, timestamp);
      if (!this.entries.has(path)) this.create(path, 1, false, timestamp);
      this.files.set(path, copy);
      this.removed.delete(path);
    }

    takeModifiedFiles() {
      const changed = [];
      for (const path of this.modified) {
        const bytes = this.files.get(path);
        if (bytes !== undefined) changed.push([path, bytes.slice()]);
      }
      this.modified.clear();
      return changed;
    }

    takeRemovedFiles() {
      const removed = [...this.removed];
      this.removed.clear();
      return removed;
    }

    exportFilesystemMetadata() {
      return {
        version: 1,
        nextInode: this.nextInode.toString(),
        clockNs: this.clockNs.toString(),
        entries: [...this.entries]
          .sort(([a], [b]) => comparePaths(a, b))
          .map(([path, entry]) => ({
            path,
            kind: entry.kind,
            mtimeNs: entry.mtimeNs.toString(),
            inode: entry.inode.toString(),
          })),
      };
    }

    takeFilesystemMetadata() {
      if (!this.metadataDirty) return null;
      const metadata = this.exportFilesystemMetadata();
      this.metadataDirty = false;
      return metadata;
    }

    importFilesystemMetadata(metadata) {
      const invalid = () => {
        throw new Error("invalid computer filesystem metadata");
      };
      const u64 = (value) => {
        if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value))
          return invalid();
        const number = BigInt(value);
        if (number > 0xffffffffffffffffn) return invalid();
        return number;
      };
      if (this.clients.size > 1 || this.openPaths.size !== 0)
        throw new Error(
          "filesystem metadata restore requires no live children or open files",
        );
      if (
        metadata?.version !== 1 ||
        !Array.isArray(metadata.entries) ||
        metadata.entries.length >
          MAX_COMPUTER_FILES + MAX_COMPUTER_DIRECTORIES + 1
      )
        invalid();
      if (
        Object.keys(metadata).some(
          (key) =>
            !["version", "nextInode", "clockNs", "entries"].includes(key),
        )
      )
        invalid();
      const nextInode = u64(metadata.nextInode);
      const clockNs = u64(metadata.clockNs);
      const entries = new Map();
      const inodes = new Set();
      let fileCount = 0;
      let directoryCount = 0;
      for (const entry of metadata.entries) {
        if (
          entry === null ||
          validPath(entry.path) === null ||
          entries.has(entry.path) ||
          (entry.kind !== 1 && entry.kind !== 2)
        )
          invalid();
        if (
          Object.keys(entry).some(
            (key) => !["path", "kind", "mtimeNs", "inode"].includes(key),
          )
        )
          invalid();
        const inode = u64(entry.inode);
        const mtimeNs = u64(entry.mtimeNs);
        if (
          inode === 0n ||
          inode >= nextInode ||
          inodes.has(inode) ||
          mtimeNs > clockNs
        )
          invalid();
        inodes.add(inode);
        entries.set(entry.path, { kind: entry.kind, inode, mtimeNs });
        if (entry.kind === 1) {
          if (!this.files.has(entry.path)) invalid();
          fileCount++;
        } else directoryCount++;
      }
      if (
        entries.get("/home")?.kind !== 2 ||
        fileCount !== this.files.size ||
        directoryCount > MAX_COMPUTER_DIRECTORIES + 1
      )
        invalid();
      for (const path of entries.keys()) {
        if (path !== "/home" && entries.get(parentPath(path))?.kind !== 2)
          invalid();
      }
      this.entries = entries;
      this.nextInode = nextInode;
      this.clockNs = clockNs;
      this.metadataDirty = false;
    }
  }

  function encodeStringRecord(strings, count = strings.length) {
    const parts = [];
    const header = new Uint8Array(4);
    new DataView(header.buffer).setUint32(0, count, true);
    parts.push(header);
    for (const value of strings) {
      const bytes = encoder.encode(value);
      const length = new Uint8Array(4);
      new DataView(length.buffer).setUint32(0, bytes.byteLength, true);
      parts.push(length, bytes);
    }
    let total = 0;
    for (const part of parts) {
      total += part.byteLength;
    }
    if (total > MAX_COMPUTER_CONTEXT_BYTES) {
      throw new Error("computer context record exceeds the host limit");
    }
    const record = new Uint8Array(total);
    let offset = 0;
    for (const part of parts) {
      record.set(part, offset);
      offset += part.byteLength;
    }
    return record;
  }

  /** Validates and encodes a computer launch context (mirror of
   * ComputerContext::new). */
  function computerContext(argumentsList, environmentPairs) {
    if (
      argumentsList.length > MAX_COMPUTER_CONTEXT_ENTRIES ||
      environmentPairs.length > MAX_COMPUTER_CONTEXT_ENTRIES
    ) {
      throw new Error("computer context entry count exceeds the host limit");
    }
    for (const argument of argumentsList) {
      if (argument.includes("\0")) {
        throw new Error("computer arguments must not contain NUL bytes");
      }
    }
    const seen = new Set();
    for (const [key, value] of environmentPairs) {
      if (!key || key.includes("=") || key.includes("\0") || seen.has(key)) {
        throw new Error(`invalid computer environment key ${key}`);
      }
      if (value.includes("\0")) {
        throw new Error("computer environment values must not contain NUL");
      }
      seen.add(key);
    }
    const flattened = [];
    for (const [key, value] of environmentPairs) {
      flattened.push(key, value);
    }
    return {
      arguments: argumentsList.slice(),
      environment: environmentPairs.map((pair) => pair.slice()),
      encodedArguments: encodeStringRecord(argumentsList),
      // The environment record counts key/value ENTRIES while flattening
      // both strings, mirroring ComputerContext's encoder.
      encodedEnvironment: encodeStringRecord(
        flattened,
        environmentPairs.length,
      ),
    };
  }

  /**
   * Generic WebSocket-to-TCP relay provider for browser Hosts.
   *
   * Protocol: the client opens `relayUrl`, sends one JSON text frame
   * `{ \"version\": 1, \"address\": \"host:port\" }`, waits for
   * `{ \"type\": \"connected\" }`, then exchanges raw TCP bytes as binary
   * frames. `{ \"type\": \"error\" }` or WebSocket closure terminates the
   * stream. TLS/HTTP/SSH remain entirely inside the guest.
   */
  class WebSocketTcpProvider {
    constructor(
      relayUrl,
      onActivity = null,
      WebSocketClass = globalThis.WebSocket,
    ) {
      if (typeof relayUrl !== "string" || relayUrl.length === 0) {
        throw new Error("TCP relay URL must be non-empty");
      }
      if (typeof WebSocketClass !== "function") {
        throw new Error("WebSocket is unavailable in this Host");
      }
      this.relayUrl = relayUrl;
      this.onActivity = onActivity;
      this.WebSocketClass = WebSocketClass;
    }

    connect(address) {
      return new WebSocketTcpSocket(
        this.relayUrl,
        address,
        this.onActivity,
        this.WebSocketClass,
      );
    }
  }

  class WebSocketTcpSocket {
    constructor(relayUrl, address, onActivity, WebSocketClass) {
      this.incoming = [];
      this.incomingOffset = 0;
      this.incomingBytes = 0;
      this.receiveFailed = false;
      this.connected = false;
      this.closed = false;
      this.onActivity = onActivity;
      this.socket = new WebSocketClass(relayUrl);
      this.socket.binaryType = "arraybuffer";
      this.socket.onopen = () => {
        if (this.closed) return;
        this.socket.send(JSON.stringify({ version: 1, address }));
      };
      this.socket.onmessage = (event) => {
        if (this.closed) return;
        if (typeof event.data === "string") {
          let message;
          try {
            message = JSON.parse(event.data);
          } catch {
            this.#terminate();
            return;
          }
          if (message?.type === "connected") {
            this.connected = true;
            this.#activity();
          } else if (message?.type === "error") {
            this.#terminate();
          }
          return;
        }
        const bytes =
          event.data instanceof ArrayBuffer
            ? new Uint8Array(event.data)
            : ArrayBuffer.isView(event.data)
              ? new Uint8Array(
                  event.data.buffer,
                  event.data.byteOffset,
                  event.data.byteLength,
                )
              : null;
        if (bytes === null) {
          this.#terminate();
          return;
        }
        if (
          !this.connected ||
          this.incomingBytes + bytes.byteLength > MAX_NET_BUFFER_BYTES ||
          (bytes.byteLength > 0 &&
            this.incoming.length >= MAX_NET_BUFFER_CHUNKS)
        ) {
          this.receiveFailed = true;
          this.incoming.length = 0;
          this.incomingOffset = 0;
          this.incomingBytes = 0;
          this.#terminate();
          return;
        }
        if (bytes.byteLength > 0) {
          this.incoming.push(bytes.slice());
          this.incomingBytes += bytes.byteLength;
        }
        this.#activity();
      };
      this.socket.onclose = () => this.#terminate();
      this.socket.onerror = () => this.#terminate();
    }

    read(capacity) {
      if (this.receiveFailed) {
        throw new Error("TCP relay receive limit or protocol violation");
      }
      if (this.incoming.length === 0) {
        return this.closed ? new Uint8Array() : null;
      }
      const output = new Uint8Array(capacity);
      let written = 0;
      while (written < capacity && this.incoming.length > 0) {
        const chunk = this.incoming[0];
        const available = chunk.byteLength - this.incomingOffset;
        const count = Math.min(available, capacity - written);
        output.set(
          chunk.subarray(this.incomingOffset, this.incomingOffset + count),
          written,
        );
        written += count;
        this.incomingBytes -= count;
        this.incomingOffset += count;
        if (this.incomingOffset === chunk.byteLength) {
          this.incoming.shift();
          this.incomingOffset = 0;
        }
      }
      return output.subarray(0, written);
    }

    write(bytes) {
      if (!this.connected) {
        if (this.closed) {
          throw new Error("TCP relay stream is closed");
        }
        return null;
      }
      // Bound browser buffering; the guest yields and retries after activity.
      if (this.socket.bufferedAmount + bytes.byteLength > MAX_NET_BUFFER_BYTES) {
        return null;
      }
      const owned = bytes.slice();
      this.socket.send(owned.buffer);
      return owned.byteLength;
    }

    close() {
      this.#terminate();
    }

    #activity() {
      this.onActivity?.();
    }

    #terminate() {
      if (this.closed) {
        return;
      }
      this.closed = true;
      this.connected = false;
      this.socket.close();
      this.#activity();
    }
  }

  /** Terminal and filesystem devices granted to one computer guest
   * (mirror of ComputerDevices). */
  class ComputerDevices {
    constructor(networkProvider = null, filesystem = new ComputerFilesystem()) {
      this.ttyInput = [];
      this.ttyInputClosed = false;
      this.ttyOutput = [];
      this.ttyColumns = 80;
      this.ttyRows = 24;
      this.ttyMode = TTY_MODE_ECHO;
      this.filesystem = filesystem;
      filesystem.clients.add(this);
      this.files = filesystem.files;
      this.modified = filesystem.modified;
      this.removed = filesystem.removed;
      this.openFiles = new Map();
      this.networkProvider = networkProvider;
      this.networkEnabled = false;
      this.sockets = new Map();
      this.nextSocket = FIRST_SOCKET_HANDLE;
    }

    setNetworkEnabled(enabled) {
      this.networkEnabled = enabled;
      if (!enabled) {
        for (const handle of this.sockets.keys()) this.netClose(handle);
      }
    }

    netTcpConnect(address) {
      if (!this.networkEnabled || this.networkProvider === null) {
        return STATUS_DENIED;
      }
      if (this.sockets.size >= MAX_OPEN_SOCKETS) {
        return STATUS_LIMIT;
      }
      let socket;
      try {
        socket = this.networkProvider.connect(address);
      } catch {
        return STATUS_INVALID;
      }
      if (
        socket === null ||
        typeof socket.read !== "function" ||
        typeof socket.write !== "function" ||
        typeof socket.close !== "function"
      ) {
        return STATUS_INVALID;
      }
      if (this.nextSocket > 0xffffffff) {
        socket.close();
        return STATUS_LIMIT;
      }
      const handle = this.nextSocket++;
      this.sockets.set(handle, socket);
      return handle;
    }

    netRead(handle, capacity) {
      const socket = this.sockets.get(handle);
      if (socket === undefined) {
        return { status: STATUS_BAD_HANDLE, bytes: new Uint8Array() };
      }
      // The embedder's socket provider sets `denied = true` on a
      // permission-gated socket whose connect request was refused.
      if (socket.denied === true) {
        return { status: STATUS_DENIED, bytes: new Uint8Array() };
      }
      try {
        const bytes = socket.read(capacity);
        if (bytes === null) {
          return { status: STATUS_WOULD_BLOCK, bytes: new Uint8Array() };
        }
        if (!(bytes instanceof Uint8Array) || bytes.byteLength > capacity) {
          return { status: STATUS_INVALID, bytes: new Uint8Array() };
        }
        return { status: bytes.byteLength, bytes };
      } catch {
        return { status: STATUS_INVALID, bytes: new Uint8Array() };
      }
    }

    netWrite(handle, bytes) {
      const socket = this.sockets.get(handle);
      if (socket === undefined) {
        return STATUS_BAD_HANDLE;
      }
      if (socket.denied === true) {
        return STATUS_DENIED;
      }
      try {
        const written = socket.write(bytes);
        if (written === null) {
          return STATUS_WOULD_BLOCK;
        }
        return Number.isInteger(written) &&
          written >= 0 &&
          written <= bytes.byteLength
          ? written
          : STATUS_INVALID;
      } catch {
        return STATUS_INVALID;
      }
    }

    netClose(handle) {
      const socket = this.sockets.get(handle);
      if (socket === undefined) {
        return STATUS_BAD_HANDLE;
      }
      this.sockets.delete(handle);
      try {
        socket.close();
        return 0;
      } catch {
        return STATUS_INVALID;
      }
    }

    pushTerminalInput(bytes) {
      if (this.ttyInputClosed) {
        throw new Error("terminal input is closed");
      }
      if (this.ttyInput.length + bytes.length > MAX_TTY_INPUT_BYTES) {
        throw new Error("terminal input queue limit exceeded");
      }
      for (const byte of bytes) {
        this.ttyInput.push(byte);
      }
    }

    closeInput() {
      this.ttyInputClosed = true;
    }

    inputSpace() {
      if (this.ttyInputClosed) {
        return 0;
      }
      return Math.max(0, MAX_TTY_INPUT_BYTES - this.ttyInput.length);
    }

    takeTerminalOutput() {
      if (this.ttyOutput.length === 0) {
        return null;
      }
      const output = Uint8Array.from(this.ttyOutput);
      this.ttyOutput.length = 0;
      return output;
    }

    mountFile(path, bytes) {
      this.filesystem.mountFile(path, bytes);
    }

    takeModifiedFiles() {
      return this.filesystem.takeModifiedFiles();
    }

    takeRemovedFiles() {
      return this.filesystem.takeRemovedFiles();
    }

    exportFilesystemMetadata() {
      return this.filesystem.exportFilesystemMetadata();
    }

    importFilesystemMetadata(metadata) {
      this.filesystem.importFilesystemMetadata(metadata);
    }

    takeFilesystemMetadata() {
      return this.filesystem.takeFilesystemMetadata();
    }

    dispose() {
      for (const handle of this.openFiles.keys()) this.fsClose(handle);
      this.filesystem.clients.delete(this);
      this.networkEnabled = false;
      for (const socket of this.sockets.values()) {
        try {
          socket.close();
        } catch {
          // Provider teardown failures must not prevent other process cleanup.
        }
      }
      this.sockets.clear();
    }

    hasTerminalInput() {
      return this.ttyInput.length > 0;
    }

    terminalMode() {
      return this.ttyMode;
    }

    ttyReadInto(handle, capacity) {
      if (handle !== COMPUTER_TTY_HANDLE) {
        return { status: STATUS_BAD_HANDLE };
      }
      if (capacity === 0) {
        return { status: STATUS_INVALID };
      }
      if (this.ttyInput.length === 0) {
        return { status: this.ttyInputClosed ? 0 : STATUS_WOULD_BLOCK };
      }
      const count = Math.min(capacity, this.ttyInput.length);
      const bytes = Uint8Array.from(this.ttyInput.slice(0, count));
      this.ttyInput.splice(0, count);
      return { status: count, bytes };
    }

    ttyWrite(handle, bytes) {
      if (handle !== COMPUTER_TTY_HANDLE) {
        return STATUS_BAD_HANDLE;
      }
      const available = Math.max(
        0,
        MAX_TTY_OUTPUT_BYTES - this.ttyOutput.length,
      );
      const written = Math.min(bytes.length, available);
      for (let index = 0; index < written; index++) {
        this.ttyOutput.push(bytes[index]);
      }
      return written;
    }

    ttySetMode(handle, flags) {
      if (handle !== COMPUTER_TTY_HANDLE) {
        return STATUS_BAD_HANDLE;
      }
      if ((flags & ~(TTY_MODE_RAW | TTY_MODE_ECHO)) !== 0) {
        return STATUS_INVALID;
      }
      this.ttyMode = flags;
      return 0;
    }

    fsOpen(path, flags) {
      if (
        validPath(path) === null ||
        !Number.isInteger(flags) ||
        flags < 0 ||
        flags > 63
      )
        return STATUS_INVALID;
      const readable = (flags & FS_OPEN_READ) !== 0;
      const writable = (flags & FS_OPEN_WRITE) !== 0;
      const create = (flags & FS_OPEN_CREATE) !== 0;
      const truncate = (flags & FS_OPEN_TRUNCATE) !== 0;
      const exclusive = (flags & FS_OPEN_EXCLUSIVE) !== 0;
      const append = (flags & FS_OPEN_APPEND) !== 0;
      if (
        (!readable && !writable) ||
        ((create || truncate || exclusive || append) && !writable) ||
        (exclusive && !create)
      )
        return STATUS_INVALID;
      const fs = this.filesystem;
      const entry = fs.entries.get(path);
      if (entry !== undefined && exclusive) return STATUS_EXISTS;
      if (entry?.kind === 2) return STATUS_IS_DIRECTORY;
      if (entry === undefined && fs.missingStatus(path) === STATUS_NOT_DIRECTORY)
        return STATUS_NOT_DIRECTORY;
      if (this.openFiles.size >= MAX_OPEN_COMPUTER_FILES) return STATUS_LIMIT;
      if (entry === undefined) {
        if (!create) return STATUS_NOT_FOUND;
        const parentStatus = fs.parentStatus(path);
        if (parentStatus !== 0) return parentStatus;
        if (this.files.size >= MAX_COMPUTER_FILES) return STATUS_LIMIT;
      }
      if (
        (entry === undefined || truncate) &&
        !fs.canMutate(entry === undefined ? 1 : 0)
      )
        return STATUS_LIMIT;
      let handle = FIRST_FILE_HANDLE;
      while (this.openFiles.has(handle)) handle++;
      if (entry === undefined || truncate) {
        this.files.set(path, new Uint8Array(0));
        if (entry === undefined) fs.create(path, 1);
        else fs.touch(path);
        this.modified.add(path);
        this.removed.delete(path);
      }
      this.openFiles.set(handle, {
        path,
        position: 0,
        readable,
        writable,
        append,
      });
      fs.openPaths.set(path, (fs.openPaths.get(path) ?? 0) + 1);
      return handle;
    }

    fsRead(handle, capacity) {
      const open = this.openFiles.get(handle);
      if (open === undefined) {
        return { status: STATUS_BAD_HANDLE };
      }
      if (!open.readable) {
        return { status: STATUS_DENIED };
      }
      const file = this.files.get(open.path);
      if (file === undefined) {
        return { status: STATUS_NOT_FOUND };
      }
      const start = Math.min(open.position, file.byteLength);
      const length = Math.min(capacity, file.byteLength - start);
      const bytes = file.slice(start, start + length);
      open.position = start + length;
      return { status: length, bytes };
    }

    fsWrite(handle, bytes) {
      const open = this.openFiles.get(handle);
      if (open === undefined) {
        return STATUS_BAD_HANDLE;
      }
      if (!open.writable) {
        return STATUS_DENIED;
      }
      const file = this.files.get(open.path);
      if (file === undefined) {
        return STATUS_NOT_FOUND;
      }
      if (bytes.byteLength === 0) return 0;
      const position = open.append ? file.byteLength : open.position;
      const end = position + bytes.byteLength;
      if (end > MAX_COMPUTER_FILE_BYTES) {
        return STATUS_LIMIT;
      }
      if (!this.filesystem.canMutate()) return STATUS_LIMIT;
      let next = file;
      if (file.byteLength < end) {
        next = new Uint8Array(end);
        next.set(file);
        this.files.set(open.path, next);
      }
      next.set(bytes, position);
      open.position = end;
      this.modified.add(open.path);
      this.filesystem.touch(open.path);
      return bytes.byteLength;
    }

    fsSeek(handle, offset, whence) {
      const open = this.openFiles.get(handle);
      if (open === undefined) {
        return STATUS_BAD_HANDLE;
      }
      const file = this.files.get(open.path);
      if (file === undefined) {
        return STATUS_NOT_FOUND;
      }
      let base;
      if (whence === 0) {
        base = 0;
      } else if (whence === 1) {
        base = open.position;
      } else if (whence === 2) {
        base = file.byteLength;
      } else {
        return STATUS_INVALID;
      }
      const position = base + offset;
      if (position < 0 || position > MAX_COMPUTER_FILE_BYTES) {
        return STATUS_INVALID;
      }
      open.position = position;
      return position;
    }

    fsTruncate(handle, length) {
      if (length > MAX_COMPUTER_FILE_BYTES) {
        return STATUS_LIMIT;
      }
      const open = this.openFiles.get(handle);
      if (open === undefined) {
        return STATUS_BAD_HANDLE;
      }
      if (!open.writable) {
        return STATUS_DENIED;
      }
      const file = this.files.get(open.path);
      if (file === undefined) {
        return STATUS_NOT_FOUND;
      }
      if (!this.filesystem.canMutate()) return STATUS_LIMIT;
      const next = new Uint8Array(length);
      next.set(file.subarray(0, Math.min(length, file.byteLength)));
      this.files.set(open.path, next);
      this.modified.add(open.path);
      this.filesystem.touch(open.path);
      return 0;
    }

    fsStat(path) {
      if (validPath(path) === null) {
        return null;
      }
      const file = this.files.get(path);
      return file === undefined ? null : file.byteLength;
    }

    fsSync(handle) {
      return this.openFiles.has(handle) ? 0 : STATUS_BAD_HANDLE;
    }

    fsClose(handle) {
      const open = this.openFiles.get(handle);
      if (open === undefined) return STATUS_BAD_HANDLE;
      this.openFiles.delete(handle);
      const count = this.filesystem.openPaths.get(open.path);
      if (count === 1) this.filesystem.openPaths.delete(open.path);
      else this.filesystem.openPaths.set(open.path, count - 1);
      return 0;
    }

    fsRemove(path) {
      if (validPath(path) === null) return STATUS_INVALID;
      const fs = this.filesystem;
      const entry = fs.entries.get(path);
      if (entry === undefined) return fs.missingStatus(path);
      if (entry.kind === 2) return STATUS_IS_DIRECTORY;
      if (fs.isOpen(path)) return STATUS_DENIED;
      if (!fs.canMutate()) return STATUS_LIMIT;
      this.files.delete(path);
      fs.entries.delete(path);
      this.modified.delete(path);
      this.removed.add(path);
      fs.touch(parentPath(path));
      return 0;
    }

    fsMkdir(path) {
      if (validPath(path) === null) return STATUS_INVALID;
      const fs = this.filesystem;
      if (fs.entries.has(path)) return STATUS_EXISTS;
      const status = fs.parentStatus(path);
      if (status !== 0) return status;
      if (fs.directoryCount() >= MAX_COMPUTER_DIRECTORIES) return STATUS_LIMIT;
      if (!fs.canMutate(1)) return STATUS_LIMIT;
      fs.create(path, 2);
      return 0;
    }

    fsRmdir(path) {
      if (validPath(path) === null) return STATUS_INVALID;
      const fs = this.filesystem;
      const entry = fs.entries.get(path);
      if (entry === undefined) return fs.missingStatus(path);
      if (entry.kind !== 2) return STATUS_NOT_DIRECTORY;
      if (path === "/home") return STATUS_DENIED;
      if (fs.isOpen(path, true)) return STATUS_DENIED;
      for (const child of fs.entries.keys()) {
        if (child.startsWith(`${path}/`)) return STATUS_NOT_EMPTY;
      }
      if (!fs.canMutate()) return STATUS_LIMIT;
      fs.entries.delete(path);
      fs.touch(parentPath(path));
      return 0;
    }

    fsRename(oldPath, newPath) {
      if (validPath(oldPath) === null || validPath(newPath) === null)
        return STATUS_INVALID;
      const fs = this.filesystem;
      const source = fs.entries.get(oldPath);
      if (source === undefined) return fs.missingStatus(oldPath);
      if (oldPath === newPath) return 0;
      if (oldPath === "/home" || newPath === "/home") return STATUS_DENIED;
      if (source.kind === 2 && newPath.startsWith(`${oldPath}/`))
        return STATUS_INVALID;
      const status = fs.parentStatus(newPath);
      if (status !== 0) return status;
      const destination = fs.entries.get(newPath);
      if (destination !== undefined && source.kind !== destination.kind) {
        return source.kind === 1 ? STATUS_IS_DIRECTORY : STATUS_NOT_DIRECTORY;
      }
      if (fs.isOpen(oldPath, true) || fs.isOpen(newPath, true))
        return STATUS_DENIED;
      if (destination?.kind === 2) {
        for (const path of fs.entries.keys()) {
          if (path.startsWith(`${newPath}/`)) return STATUS_NOT_EMPTY;
        }
      }
      const moves = [];
      for (const [path, entry] of fs.entries) {
        if (path === oldPath || path.startsWith(`${oldPath}/`)) {
          const target = newPath + path.slice(oldPath.length);
          if (validPath(target) === null) return STATUS_INVALID;
          moves.push([path, target, entry, this.files.get(path)]);
        }
      }
      if (!fs.canMutate()) return STATUS_LIMIT;
      fs.entries.delete(newPath);
      for (const [path, , , bytes] of moves) {
        fs.entries.delete(path);
        if (bytes !== undefined) {
          this.files.delete(path);
          this.modified.delete(path);
          this.removed.add(path);
        }
      }
      for (const [, target, entry, bytes] of moves) {
        fs.entries.set(target, entry);
        if (bytes !== undefined) {
          this.files.set(target, bytes);
          this.modified.add(target);
          this.removed.delete(target);
        }
      }
      const timestamp = fs.tick();
      fs.touch(parentPath(oldPath), timestamp);
      if (parentPath(oldPath) !== parentPath(newPath))
        fs.touch(parentPath(newPath), timestamp);
      return 0;
    }

    fsMetadata(path) {
      if (validPath(path) === null) return { status: STATUS_INVALID };
      const entry = this.filesystem.entries.get(path);
      if (entry === undefined) return { status: this.filesystem.missingStatus(path) };
      const record = new Uint8Array(24);
      const view = new DataView(record.buffer);
      view.setUint32(0, entry.kind, true);
      view.setUint32(4, this.files.get(path)?.byteLength ?? 0, true);
      view.setBigUint64(8, entry.mtimeNs, true);
      view.setBigUint64(16, entry.inode, true);
      return { status: 0, record };
    }

    fsFstat(handle) {
      const open = this.openFiles.get(handle);
      return open === undefined
        ? { status: STATUS_BAD_HANDLE }
        : this.fsMetadata(open.path);
    }

    fsListDirectory(path) {
      if (validPath(path) === null) return { status: STATUS_INVALID };
      const fs = this.filesystem;
      const entry = fs.entries.get(path);
      if (entry === undefined) return { status: fs.missingStatus(path) };
      if (entry.kind !== 2) return { status: STATUS_NOT_DIRECTORY };
      const children = [...fs.entries]
        .filter(([child]) => child !== "/home" && parentPath(child) === path)
        .sort(([a], [b]) => comparePaths(a, b))
        .map(([child, metadata]) => [
          encoder.encode(child.slice(path.length + 1)),
          metadata.kind,
        ]);
      const record = new Uint8Array(
        4 + children.reduce((size, [name]) => size + 8 + name.length, 0),
      );
      const view = new DataView(record.buffer);
      view.setUint32(0, children.length, true);
      let offset = 4;
      for (const [name, kind] of children) {
        view.setUint32(offset, name.length, true);
        record.set(name, offset + 4);
        view.setUint32(offset + 4 + name.length, kind, true);
        offset += 8 + name.length;
      }
      return { status: record.byteLength, record };
    }

    fsListRecord() {
      return encodeStringRecord([...this.files.keys()].sort(comparePaths));
    }
  }

  /** One translated computer guest process (mirror of ComputerRuntime). */
  class ComputerProcess {
    constructor(
      program,
      context,
      maxGas,
      emitLog = null,
      networkProvider = null,
      filesystem = new ComputerFilesystem(),
    ) {
      this.metadata = readMetadata(program.module);
      this.instance = new WebAssembly.Instance(program.module, {});
      this.partInstances = program.parts.map(
        (part) => new WebAssembly.Instance(part, { pvm: this.instance.exports }),
      );
      this.pvm = this.instance.exports;
      this.memory = this.pvm.memory;
      this.maxGas = BigInt(maxGas);
      this.emitLog = emitLog;
      this.encodedArguments = context.encodedArguments;
      this.encodedEnvironment = context.encodedEnvironment;
      this.exitStatus = null;
      this.pendingSpawn = null;
      this.pendingChildRequest = null;
      this.resumePending = false;
      this.monotonicEpoch = performance.now();
      const start = this.metadata.exports.get("_pvm_start");
      if (start === undefined) {
        throw new Error("computer guests must export '_pvm_start'");
      }
      this.startBlock = start;
      this.#setup(context);
      this.devices = new ComputerDevices(networkProvider, filesystem);
    }

    /** Mirrors Vm::setup: argc/argv/envp/auxv stack for `_pvm_start`. */
    #setup(context) {
      const argc = BigInt(context.arguments.length);
      const envpLength = BigInt(context.environment.length);
      const auxv = [[AT_PAGESZ, 4096n]];
      let sp = BigInt(this.metadata.layout.stackHigh);
      sp -=
        (1n + argc + 1n + envpLength + 1n + BigInt(auxv.length + 1) * 2n) * 8n;
      const addressInit = sp;
      let pointer = sp;
      this.#writeU64(pointer, argc);
      pointer += 8n;
      for (const argument of context.arguments) {
        const bytes = encoder.encode(argument);
        sp -= BigInt(bytes.byteLength) + 1n;
        this.#write(Number(sp), bytes);
        this.#writeU64(pointer, sp);
        pointer += 8n;
      }
      pointer += 8n; // Null pointer.
      for (const [key, value] of context.environment) {
        const bytes = encoder.encode(`${key}=${value}`);
        sp -= BigInt(bytes.byteLength) + 1n;
        this.#write(Number(sp), bytes);
        this.#writeU64(pointer, sp);
        pointer += 8n;
      }
      pointer += 8n; // Null pointer.
      for (const [key, value] of auxv) {
        this.#writeU64(pointer, key);
        pointer += 8n;
        this.#writeU64(pointer, value);
        pointer += 8n;
      }
      this.#setReg(1, sp); // SP
      this.#setReg(7, addressInit); // A0
    }

    /** Runs until the guest yields, exits, or requests supervision.
     * Faults (trap, out of gas) throw, mirroring the native Err path. */
    run() {
      try {
        return this.#run();
      } catch (error) {
        this.dispose();
        throw error;
      }
    }

    dispose() {
      this.devices.dispose();
      if (this.exitStatus === null) this.exitStatus = 130;
    }

    #run() {
      if (this.exitStatus !== null) {
        return { kind: "exited", code: this.exitStatus };
      }
      let status;
      if (this.resumePending) {
        this.resumePending = false;
        this.pvm.pvm_set_gas(this.maxGas);
        status = this.pvm.pvm_resume();
      } else {
        status = this.pvm.pvm_begin(this.startBlock, this.maxGas);
      }
      for (;;) {
        if (status === STATUS_TRAP) {
          throw new Error(
            `computer guest trapped at ${this.pvm.trap_pc.value}`,
          );
        }
        if (status === STATUS_OUT_OF_GAS) {
          throw new Error("computer guest ran out of gas");
        }
        if (status === STATUS_FINISHED) {
          throw new Error("computer guest returned without exiting");
        }
        if (status !== STATUS_ECALL) {
          throw new Error(`translated PolkaVM returned status ${status}`);
        }
        const name = this.metadata.imports[this.pvm.ecall.value >>> 0];
        const outcome = this.#hostcall(name);
        if (outcome === "continue") {
          status = this.pvm.pvm_resume();
          continue;
        }
        this.resumePending = true;
        if (outcome === "yield") {
          return { kind: "yielded" };
        }
        if (outcome === "exit") {
          this.dispose();
          return { kind: "exited", code: this.exitStatus };
        }
        if (outcome === "spawn") {
          return { kind: "spawn" };
        }
        return { kind: "child" };
      }
    }

    /** Completes a suspended process/pipe hostcall with a status value. */
    resolveSpawn(result) {
      this.#setReg(7, BigInt(result));
    }

    /** Completes a suspended pipe_read by delivering bytes. */
    resolveRead(destination, bytes) {
      this.#write(destination, bytes);
      this.#setReg(7, BigInt(bytes.byteLength));
    }

    takeSpawnRequest() {
      const request = this.pendingSpawn;
      this.pendingSpawn = null;
      return request;
    }

    takeChildRequest() {
      const request = this.pendingChildRequest;
      this.pendingChildRequest = null;
      return request;
    }

    sendTerminalInput(bytes) {
      this.devices.pushTerminalInput(bytes);
    }

    takeTerminalOutput() {
      return this.devices.takeTerminalOutput();
    }

    hasTerminalInput() {
      return this.devices.hasTerminalInput();
    }

    terminalInputClosed() {
      return this.devices.ttyInputClosed;
    }

    terminalInputSpace() {
      return this.devices.inputSpace();
    }

    closeTerminalInput() {
      this.devices.closeInput();
    }

    setNetworkEnabled(enabled) {
      this.devices.setNetworkEnabled(enabled);
    }

    setTerminalSize(columns, rows) {
      if (!columns || !rows || columns > 1000 || rows > 1000) {
        throw new Error(`invalid terminal size ${columns}x${rows}`);
      }
      this.devices.ttyColumns = columns;
      this.devices.ttyRows = rows;
    }

    terminalMode() {
      return this.devices.terminalMode();
    }

    mountFile(path, bytes) {
      this.devices.mountFile(path, bytes);
    }

    takeModifiedFiles() {
      return this.devices.takeModifiedFiles();
    }

    takeRemovedFiles() {
      return this.devices.takeRemovedFiles();
    }

    exportFilesystemMetadata() {
      return this.devices.exportFilesystemMetadata();
    }

    importFilesystemMetadata(metadata) {
      this.devices.importFilesystemMetadata(metadata);
    }

    takeFilesystemMetadata() {
      return this.devices.takeFilesystemMetadata();
    }

    // eslint-disable-next-line complexity -- Flat dispatch mirrors the ABI.
    #hostcall(name) {
      const a0 = this.#reg(7);
      const a1 = this.#reg(8);
      const a2 = this.#reg(9);
      const a3 = this.#reg(10);
      switch (name) {
        case "polkadot_host_0_1_core_yield":
          return "yield";
        case "polkadot_host_0_1_core_exit":
          this.exitStatus = Number(BigInt.asIntN(32, a0));
          return "exit";
        case "polkadot_host_0_1_core_args":
          this.#writeRecord(a0, a1, this.encodedArguments);
          return "continue";
        case "polkadot_host_0_1_core_environment":
          this.#writeRecord(a0, a1, this.encodedEnvironment);
          return "continue";
        case "polkadot_host_0_1_core_clock_monotonic": {
          const record = new Uint8Array(8);
          new DataView(record.buffer).setBigUint64(
            0,
            BigInt(Math.floor((performance.now() - this.monotonicEpoch) * 1e6)),
            true,
          );
          this.#write(this.#u32(a0), record);
          this.#setReg(7, 0n);
          return "continue";
        }
        case "polkadot_host_0_1_core_clock_wall": {
          const record = new Uint8Array(8);
          new DataView(record.buffer).setBigUint64(
            0,
            BigInt(Date.now()) * 1_000_000n,
            true,
          );
          this.#write(this.#u32(a0), record);
          this.#setReg(7, 0n);
          return "continue";
        }
        case "polkadot_host_0_1_core_random": {
          const length = this.#u32(a1);
          if (length === 0) {
            this.#setReg(7, BigInt(STATUS_INVALID));
          } else if (length > MAX_RANDOM_BYTES) {
            this.#setReg(7, BigInt(STATUS_LIMIT));
          } else {
            const bytes = new Uint8Array(length);
            crypto.getRandomValues(bytes);
            this.#write(this.#u32(a0), bytes);
            this.#setReg(7, 0n);
          }
          return "continue";
        }
        case "polkadot_host_0_1_tty_current":
          this.#setReg(7, BigInt(COMPUTER_TTY_HANDLE));
          return "continue";
        case "polkadot_host_0_1_tty_read": {
          const capacity = Math.min(this.#u32(a2), MAX_TTY_TRANSFER);
          if (capacity === 0) {
            this.#setReg(7, BigInt(STATUS_INVALID));
            return "continue";
          }
          const result = this.devices.ttyReadInto(this.#u32(a0), capacity);
          if (result.status > 0) {
            this.#write(this.#u32(a1), result.bytes);
          }
          this.#setReg(7, BigInt(result.status));
          return "continue";
        }
        case "polkadot_host_0_1_tty_write": {
          const length = this.#u32(a2);
          if (length > MAX_TTY_TRANSFER) {
            this.#setReg(7, BigInt(STATUS_LIMIT));
            return "continue";
          }
          const bytes = this.#read(this.#u32(a1), length);
          this.#setReg(7, BigInt(this.devices.ttyWrite(this.#u32(a0), bytes)));
          return "continue";
        }
        case "polkadot_host_0_1_tty_get_size": {
          if (this.#u32(a0) !== COMPUTER_TTY_HANDLE) {
            this.#setReg(7, BigInt(STATUS_BAD_HANDLE));
            return "continue";
          }
          const record = new Uint8Array(8);
          const view = new DataView(record.buffer);
          view.setUint32(0, this.devices.ttyColumns, true);
          view.setUint32(4, this.devices.ttyRows, true);
          this.#write(this.#u32(a1), record);
          this.#setReg(7, 0n);
          return "continue";
        }
        case "polkadot_host_0_1_tty_set_mode":
          this.#setReg(
            7,
            BigInt(this.devices.ttySetMode(this.#u32(a0), this.#u32(a1))),
          );
          return "continue";
        case "polkadot_host_0_1_fs_open": {
          const path = this.#readPath(a0, a1);
          this.#setReg(
            7,
            BigInt(
              path === null
                ? STATUS_INVALID
                : this.devices.fsOpen(path, this.#u32(a2)),
            ),
          );
          return "continue";
        }
        case "polkadot_host_0_1_fs_read": {
          const capacity = Math.min(this.#u32(a2), MAX_FS_TRANSFER);
          const result = this.devices.fsRead(this.#u32(a0), capacity);
          if (result.status > 0) {
            this.#write(this.#u32(a1), result.bytes);
          }
          this.#setReg(7, BigInt(result.status));
          return "continue";
        }
        case "polkadot_host_0_1_fs_write": {
          const length = this.#u32(a2);
          if (length > MAX_FS_TRANSFER) {
            this.#setReg(7, BigInt(STATUS_LIMIT));
            return "continue";
          }
          const bytes = this.#read(this.#u32(a1), length);
          this.#setReg(7, BigInt(this.devices.fsWrite(this.#u32(a0), bytes)));
          return "continue";
        }
        case "polkadot_host_0_1_fs_seek":
          this.#setReg(
            7,
            BigInt(
              this.devices.fsSeek(
                this.#u32(a0),
                Number(BigInt.asIntN(32, a1)),
                this.#u32(a2),
              ),
            ),
          );
          return "continue";
        case "polkadot_host_0_1_fs_truncate":
          this.#setReg(
            7,
            BigInt(this.devices.fsTruncate(this.#u32(a0), this.#u32(a1))),
          );
          return "continue";
        case "polkadot_host_0_1_fs_stat": {
          const path = this.#readPath(a0, a1);
          const size = path === null ? null : this.devices.fsStat(path);
          if (size === null) {
            this.#setReg(7, BigInt(STATUS_NOT_FOUND));
            return "continue";
          }
          const record = new Uint8Array(4);
          new DataView(record.buffer).setUint32(0, size, true);
          this.#write(this.#u32(a2), record);
          this.#setReg(7, 0n);
          return "continue";
        }
        case "polkadot_host_0_1_fs_metadata":
        case "polkadot_host_0_1_fs_fstat": {
          const byHandle = name === "polkadot_host_0_1_fs_fstat";
          const result = byHandle
            ? this.devices.fsFstat(this.#u32(a0))
            : this.devices.fsMetadata(this.#readPath(a0, a1));
          if (result.status === 0)
            this.#write(this.#u32(byHandle ? a1 : a2), result.record);
          this.#setReg(7, BigInt(result.status));
          return "continue";
        }
        case "polkadot_host_0_1_fs_mkdir":
        case "polkadot_host_0_1_fs_rmdir": {
          const path = this.#readPath(a0, a1);
          const status =
            name === "polkadot_host_0_1_fs_mkdir"
              ? this.devices.fsMkdir(path)
              : this.devices.fsRmdir(path);
          this.#setReg(7, BigInt(status));
          return "continue";
        }
        case "polkadot_host_0_1_fs_rename":
          this.#setReg(
            7,
            BigInt(
              this.devices.fsRename(
                this.#readPath(a0, a1),
                this.#readPath(a2, a3),
              ),
            ),
          );
          return "continue";
        case "polkadot_host_0_1_fs_list_directory": {
          const result = this.devices.fsListDirectory(this.#readPath(a0, a1));
          if (result.status < 0) this.#setReg(7, BigInt(result.status));
          else if (result.record.byteLength > this.#u32(a3))
            this.#setReg(7, BigInt(-result.record.byteLength));
          else {
            this.#write(this.#u32(a2), result.record);
            this.#setReg(7, BigInt(result.record.byteLength));
          }
          return "continue";
        }
        case "polkadot_host_0_1_fs_sync":
          this.#setReg(7, BigInt(this.devices.fsSync(this.#u32(a0))));
          return "continue";
        case "polkadot_host_0_1_fs_close":
          this.#setReg(7, BigInt(this.devices.fsClose(this.#u32(a0))));
          return "continue";
        case "polkadot_host_0_1_fs_remove": {
          const path = this.#readPath(a0, a1);
          this.#setReg(
            7,
            BigInt(
              path === null ? STATUS_INVALID : this.devices.fsRemove(path),
            ),
          );
          return "continue";
        }
        case "polkadot_host_0_1_fs_list": {
          const capacity = Math.min(this.#u32(a1), MAX_FS_TRANSFER);
          const record = this.devices.fsListRecord();
          if (record.byteLength > capacity) {
            this.#setReg(7, BigInt(-record.byteLength));
          } else {
            this.#write(this.#u32(a0), record);
            this.#setReg(7, BigInt(record.byteLength));
          }
          return "continue";
        }
        case "polkadot_host_0_1_process_run":
        case "polkadot_host_0_1_process_spawn": {
          const pkg = this.#readString(a0, a1, 64);
          const argumentsList =
            pkg === null ? null : this.#readStringRecord(a2, a3);
          if (pkg === null || argumentsList === null) {
            this.#setReg(7, BigInt(STATUS_INVALID));
            return "continue";
          }
          if (name === "polkadot_host_0_1_process_run") {
            this.pendingSpawn = { package: pkg, arguments: argumentsList };
            return "spawn";
          }
          this.pendingChildRequest = {
            kind: "spawn",
            package: pkg,
            arguments: argumentsList,
          };
          return "child";
        }
        case "polkadot_host_0_1_process_wait":
          this.pendingChildRequest = { kind: "wait", pid: this.#u32(a0) };
          return "child";
        case "polkadot_host_0_1_pipe_read": {
          const capacity = Math.min(this.#u32(a2), MAX_TTY_TRANSFER);
          if (capacity === 0) {
            this.#setReg(7, BigInt(STATUS_INVALID));
            return "continue";
          }
          this.pendingChildRequest = {
            kind: "pipeRead",
            pid: this.#u32(a0),
            destination: this.#u32(a1),
            capacity,
          };
          return "child";
        }
        case "polkadot_host_0_1_pipe_write": {
          const length = Math.min(this.#u32(a2), MAX_TTY_TRANSFER);
          const bytes = this.#read(this.#u32(a1), length);
          this.pendingChildRequest = {
            kind: "pipeWrite",
            pid: this.#u32(a0),
            bytes,
          };
          return "child";
        }
        case "polkadot_host_0_1_pipe_close":
          this.pendingChildRequest = { kind: "pipeClose", pid: this.#u32(a0) };
          return "child";
        case "polkadot_host_0_1_workspace_spawn": {
          const a4 = this.#reg(11);
          const a5 = this.#reg(12);
          const pkg = this.#readString(a0, a1, 64);
          const argumentsList =
            pkg === null ? null : this.#readStringRecord(a2, a3);
          if (
            pkg === null ||
            argumentsList === null ||
            a4 > 0xffffffffn ||
            a5 > 0xffffffffn
          ) {
            this.#setReg(7, BigInt(STATUS_INVALID));
            return "continue";
          }
          this.pendingChildRequest = {
            kind: "workspaceSpawn",
            package: pkg,
            arguments: argumentsList,
            columns: this.#u32(a4),
            rows: this.#u32(a5),
          };
          return "child";
        }
        case "polkadot_host_0_1_workspace_send_input": {
          const length = Math.min(this.#u32(a2), MAX_TTY_TRANSFER);
          this.pendingChildRequest = {
            kind: "workspaceSendInput",
            handle: this.#u32(a0),
            bytes: this.#read(this.#u32(a1), length),
          };
          return "child";
        }
        case "polkadot_host_0_1_workspace_read": {
          const capacity = Math.min(this.#u32(a2), MAX_TTY_TRANSFER);
          if (capacity === 0) {
            this.#setReg(7, BigInt(STATUS_INVALID));
            return "continue";
          }
          this.pendingChildRequest = {
            kind: "workspaceRead",
            handle: this.#u32(a0),
            destination: this.#u32(a1),
            capacity,
          };
          return "child";
        }
        case "polkadot_host_0_1_workspace_resize":
          this.pendingChildRequest = {
            kind: "workspaceResize",
            handle: this.#u32(a0),
            columns: this.#u32(a1),
            rows: this.#u32(a2),
          };
          return "child";
        case "polkadot_host_0_1_workspace_wait":
          this.pendingChildRequest = {
            kind: "workspaceWait",
            handle: this.#u32(a0),
          };
          return "child";
        case "polkadot_host_0_1_workspace_close":
          this.pendingChildRequest = {
            kind: "workspaceClose",
            handle: this.#u32(a0),
          };
          return "child";
        case "polkadot_host_0_1_net_tcp_connect": {
          const address = this.#readString(a0, a1, MAX_NET_ADDRESS_BYTES);
          this.#setReg(
            7,
            BigInt(
              address === null
                ? STATUS_INVALID
                : this.devices.netTcpConnect(address),
            ),
          );
          return "continue";
        }
        case "polkadot_host_0_1_net_read": {
          const capacity = Math.min(this.#u32(a2), MAX_TTY_TRANSFER);
          const result = this.devices.netRead(this.#u32(a0), capacity);
          if (result.status > 0) {
            this.#write(this.#u32(a1), result.bytes);
          }
          this.#setReg(7, BigInt(result.status));
          return "continue";
        }
        case "polkadot_host_0_1_net_write": {
          const length = this.#u32(a2);
          if (length > MAX_TTY_TRANSFER) {
            this.#setReg(7, BigInt(STATUS_LIMIT));
          } else {
            this.#setReg(
              7,
              BigInt(
                this.devices.netWrite(
                  this.#u32(a0),
                  this.#read(this.#u32(a1), length),
                ),
              ),
            );
          }
          return "continue";
        }
        case "polkadot_host_0_1_net_close":
          this.#setReg(7, BigInt(this.devices.netClose(this.#u32(a0))));
          return "continue";
        case "host_log": {
          const length = Math.min(this.#u32(a1), 4096);
          if (this.emitLog) {
            try {
              this.emitLog(
                new TextDecoder().decode(this.#read(this.#u32(a0), length)),
              );
            } catch {
              // Diagnostics only.
            }
          }
          return "continue";
        }
        default:
          throw new Error(`unsupported import: ${name}`);
      }
    }

    #writeRecord(pointer, capacity, record) {
      const available = this.#u32(capacity);
      if (record.byteLength > available) {
        this.#setReg(7, BigInt(-record.byteLength));
        return;
      }
      this.#write(this.#u32(pointer), record);
      this.#setReg(7, BigInt(record.byteLength));
    }

    #readPath(pointer, length) {
      return this.#readString(pointer, length, MAX_COMPUTER_PATH_BYTES);
    }

    #readString(pointer, length, maximum) {
      const count = this.#u32(length);
      if (count === 0 || count > maximum) {
        return null;
      }
      try {
        return decoder.decode(this.#read(this.#u32(pointer), count));
      } catch {
        return null;
      }
    }

    #readStringRecord(pointer, length) {
      const count = this.#u32(length);
      if (count > 4096) {
        return null;
      }
      if (count === 0) {
        return [];
      }
      const bytes = this.#read(this.#u32(pointer), count);
      if (bytes.byteLength < 4) {
        return null;
      }
      const view = new DataView(bytes.buffer, bytes.byteOffset);
      const entries = view.getUint32(0, true);
      if (entries > 16) {
        return null;
      }
      const output = [];
      let cursor = 4;
      for (let index = 0; index < entries; index++) {
        if (cursor + 4 > bytes.byteLength) {
          return null;
        }
        const entryLength = view.getUint32(cursor, true);
        cursor += 4;
        if (cursor + entryLength > bytes.byteLength) {
          return null;
        }
        try {
          output.push(
            decoder.decode(bytes.subarray(cursor, cursor + entryLength)),
          );
        } catch {
          return null;
        }
        cursor += entryLength;
      }
      return output;
    }

    #reg(index) {
      return BigInt.asUintN(64, this.pvm[`r${index}`].value);
    }

    #setReg(index, value) {
      this.pvm[`r${index}`].value = this.metadata.is64Bit
        ? BigInt.asIntN(64, value)
        : BigInt.asUintN(32, value);
    }

    #u32(value) {
      return Number(value & 0xffffffffn) >>> 0;
    }

    #range(address, length, write = false) {
      address >>>= 0;
      length >>>= 0;
      const end = address + length;
      if (end > 0x100000000) {
        throw new Error("computer guest memory access is out of range");
      }
      const { layout } = this.metadata;
      let physical;
      if (address >= layout.stackLow && end <= layout.stackHigh) {
        physical = layout.stackPhysical + address - layout.stackLow;
      } else if (
        address >= layout.rwAddress &&
        end <=
          layout.rwAddress + this.memory.buffer.byteLength - layout.rwPhysical
      ) {
        physical = layout.rwPhysical + address - layout.rwAddress;
      } else if (
        !write &&
        address >= layout.roAddress &&
        end <= layout.roAddress + layout.roSize
      ) {
        physical = layout.roPhysical + address - layout.roAddress;
      } else {
        throw new Error("computer guest memory access is out of range");
      }
      return new Uint8Array(this.memory.buffer, physical, length);
    }

    #read(address, length) {
      return this.#range(address, length).slice();
    }

    #write(address, bytes) {
      this.#range(address, bytes.byteLength, true).set(bytes);
    }

    #writeU64(address, value) {
      const bytes = this.#range(Number(address) >>> 0, 8, true);
      new DataView(bytes.buffer, bytes.byteOffset, 8).setBigUint64(
        0,
        BigInt.asUintN(64, value),
        true,
      );
    }
  }

  /** Supervises computer processes sharing one terminal and `/home`
   * (mirror of ComputerSupervisor, including the hardening semantics). */
  class ComputerSupervisor {
    constructor(module, context, maxGas, emitLog = null, options = null) {
      this.packages = new Map();
      this.networkProvider = options?.networkProvider ?? null;
      this.filesystem = options?.filesystem ?? new ComputerFilesystem();
      this.stack = [
        new ComputerProcess(
          module,
          context,
          maxGas,
          emitLog,
          this.networkProvider,
          this.filesystem,
        ),
      ];
      this.background = [];
      this.workspaceChildren = [];
      this.nextPid = 2;
      this.pendingOutput = [];
      this.environment = context.environment.map((pair) => pair.slice());
      this.maxGas = maxGas;
      this.emitLog = emitLog;
      this.network = false;
      this.workspace = false;
      this.columns = 80;
      this.rows = 24;
      // Open spawn: when enabled, a spawn naming an unregistered package
      // suspends the computer with { kind: "package" } instead of failing
      // with NOT_FOUND, so the embedding Host can resolve the name (e.g.
      // through DotNS), then providePackage()/rejectPackage(). Disabled by
      // default: the conformance contract expects immediate NOT_FOUND.
      this.packageResolution = options?.packageResolution === true;
      this.pendingResolution = null;
    }

    /** Registers the pending package and retries the suspended spawn. */
    providePackage(module) {
      const pending = this.pendingResolution;
      if (pending === null) {
        throw new Error("no package resolution is pending");
      }
      this.pendingResolution = null;
      if (pending.mode === "childRoute") {
        // Share the resolution with the whole tree, then route it to the
        // suspended child.
        const child = this.workspaceChildren.find(
          (entry) => entry.handle === pending.handle,
        );
        if (child === undefined) {
          throw new Error("suspended workspace child is gone");
        }
        const name = child.supervisor.pendingPackage();
        if (name !== null) {
          this.registerPackage(name, module);
        }
        child.supervisor.providePackage(module);
        return;
      }
      this.registerPackage(pending.request.package, module);
      this.#dispatchSpawn(pending);
    }

    /** Fails the suspended spawn; the guest observes the status. */
    rejectPackage(status = STATUS_NOT_FOUND) {
      const pending = this.pendingResolution;
      if (pending === null) {
        throw new Error("no package resolution is pending");
      }
      this.pendingResolution = null;
      if (pending.mode === "childRoute") {
        const child = this.workspaceChildren.find(
          (entry) => entry.handle === pending.handle,
        );
        if (child === undefined) {
          throw new Error("suspended workspace child is gone");
        }
        child.supervisor.rejectPackage(status);
        return;
      }
      this.#foreground().resolveSpawn(status);
    }

    /** Returns the package name awaiting embedder resolution, if any. */
    pendingPackage() {
      const pending = this.pendingResolution;
      if (pending === null) {
        return null;
      }
      if (pending.mode === "childRoute") {
        const child = this.workspaceChildren.find(
          (entry) => entry.handle === pending.handle,
        );
        return child ? child.supervisor.pendingPackage() : null;
      }
      return pending.request.package;
    }

    #dispatchSpawn(pending) {
      if (pending.mode === "child") {
        this.#handleChildRequest(pending.request);
        return;
      }
      if (pending.mode === "workspace") {
        this.#handleWorkspaceRequest(pending.request, (value) =>
          this.#foreground().resolveSpawn(value),
        );
        return;
      }
      const child = this.#spawnChild(
        pending.request.package,
        pending.request.arguments,
      );
      if (typeof child === "number") {
        this.#foreground().resolveSpawn(child);
      } else {
        this.stack.push(child);
      }
    }

    #suspendForPackage(request, mode) {
      this.pendingResolution = { request, mode };
      return { kind: "package", package: request.package };
    }

    registerPackage(name, module) {
      if (!name || name.length > 64 || !/^[A-Za-z0-9._-]+$/.test(name)) {
        throw new Error(`invalid package name ${name}`);
      }
      this.packages.set(name, module);
    }

    /** Mounts one persistent file into the shared `/home` store. The mount
     * reaches every live process — foreground stack, piped children, and
     * workspace children — so a file provided after a child spawned (e.g.
     * the seeds of an open-resolved package) is visible tree-wide. */
    mountFile(path, bytes) {
      this.filesystem.mountFile(path, bytes);
    }

    setNetworkEnabled(enabled) {
      if (enabled && this.networkProvider === null) {
        throw new Error("network capability requires a Host network provider");
      }
      for (const process of this.stack) {
        process.setNetworkEnabled(enabled);
      }
      for (const child of this.background) {
        child.process.setNetworkEnabled(enabled);
      }
      for (const child of this.workspaceChildren) {
        child.supervisor.setNetworkEnabled(enabled);
      }
      this.network = enabled;
    }

    /** Grants or revokes the workspace capability for the root process.
     * Revocation reaps every live workspace child. */
    setWorkspaceEnabled(enabled) {
      this.workspace = enabled;
      if (!enabled) {
        for (const child of this.workspaceChildren) child.supervisor.dispose();
        this.workspaceChildren.length = 0;
        if (this.pendingResolution?.mode === "workspace") {
          this.pendingResolution = null;
          this.#foreground().resolveSpawn(STATUS_DENIED);
        } else if (this.pendingResolution?.mode === "childRoute") {
          this.pendingResolution = null;
        }
      }
    }

    setTerminalSize(columns, rows) {
      for (const process of this.stack) {
        process.setTerminalSize(columns, rows);
      }
      for (const child of this.background) {
        child.process.setTerminalSize(columns, rows);
      }
      this.columns = columns;
      this.rows = rows;
    }

    sendTerminalInput(bytes) {
      this.#foreground().sendTerminalInput(bytes);
    }

    terminalInputSpace() {
      return this.#foreground().terminalInputSpace();
    }

    takeTerminalOutput() {
      if (this.pendingOutput.length > 0) {
        const output = Uint8Array.from(this.pendingOutput);
        this.pendingOutput.length = 0;
        return output;
      }
      return this.#foreground().takeTerminalOutput();
    }

    hasTerminalInput() {
      return this.#foreground().hasTerminalInput();
    }

    terminalMode() {
      return this.#foreground().terminalMode();
    }

    takeModifiedFiles() {
      return this.filesystem.takeModifiedFiles();
    }

    takeRemovedFiles() {
      return this.filesystem.takeRemovedFiles();
    }

    exportFilesystemMetadata() {
      return this.filesystem.exportFilesystemMetadata();
    }

    importFilesystemMetadata(metadata) {
      this.filesystem.importFilesystemMetadata(metadata);
    }

    takeFilesystemMetadata() {
      return this.filesystem.takeFilesystemMetadata();
    }

    dispose() {
      for (const process of this.stack) process.dispose();
      for (const child of this.background) child.process.dispose();
      for (const child of this.workspaceChildren) child.supervisor.dispose();
      this.background.length = 0;
      this.workspaceChildren.length = 0;
      this.pendingResolution = null;
    }

    /** Runs the foreground process until the system yields or the root
     * exits; child faults fail only the child (status 139). */
    run() {
      try {
        return this.#run();
      } catch (error) {
        this.dispose();
        throw error;
      }
    }


    #run() {
      if (this.pendingResolution !== null) {
        // Idempotent while suspended: the embedder must provide or reject
        // the pending package before execution can continue.
        return { kind: "package", package: this.pendingPackage() };
      }
      let faultPops = 0;
      for (;;) {
        let status;
        try {
          status = this.#foreground().run();
        } catch (error) {
          if (this.stack.length === 1) {
            throw error;
          }
          faultPops++;
          if (faultPops > MAX_FAULT_POPS_PER_RUN) {
            throw new Error(`children faulted repeatedly: ${error.message}`);
          }
          this.#popForeground(FAULTED_CHILD_STATUS);
          continue;
        }
        if (status.kind === "yielded") {
          // Surface a suspended workspace child's resolution once the
          // workspace guest has yielded; the embedder resolves it before
          // execution continues anywhere in the tree.
          const suspended = this.workspaceChildren.find(
            (child) =>
              child.exit === null && child.supervisor.pendingPackage() !== null,
          );
          if (suspended !== undefined) {
            this.pendingResolution = {
              mode: "childRoute",
              handle: suspended.handle,
            };
            return { kind: "package", package: this.pendingPackage() };
          }
          return { kind: "yielded" };
        }
        if (status.kind === "spawn") {
          const request = this.#foreground().takeSpawnRequest();
          if (!request) {
            throw new Error("spawn status without a pending request");
          }
          const child = this.#spawnChild(request.package, request.arguments);
          if (child === STATUS_NOT_FOUND && this.packageResolution) {
            return this.#suspendForPackage(request, "stack");
          }
          if (typeof child === "number") {
            this.#foreground().resolveSpawn(child);
          } else {
            this.stack.push(child);
          }
          continue;
        }
        if (status.kind === "child") {
          const request = this.#foreground().takeChildRequest();
          if (!request) {
            throw new Error("child-request status without a pending request");
          }
          this.#handleChildRequest(request);
          if (this.pendingResolution !== null) {
            return { kind: "package", package: this.pendingPackage() };
          }
          continue;
        }
        // Exited.
        if (this.stack.length === 1) {
          this.dispose();
          return { kind: "exited", code: status.code };
        }
        this.#popForeground(status.code & 0xff);
      }
    }

    /** Host-authority cancellation of the foreground process. */
    terminateForeground() {
      if (this.stack.length === 1) {
        const root = this.#foreground();
        if (root.exitStatus === null) {
          root.exitStatus = 130;
        }
        this.dispose();
        return { kind: "exited", code: root.exitStatus };
      }
      this.pendingResolution = null;
      this.#popForeground(130);
      return { kind: "yielded" };
    }

    #foreground() {
      return this.stack[this.stack.length - 1];
    }

    #appendPendingOutput(bytes) {
      const available = MAX_TTY_OUTPUT_BYTES - this.pendingOutput.length;
      const count = Math.min(bytes.byteLength, Math.max(0, available));
      for (let index = 0; index < count; index++) {
        this.pendingOutput.push(bytes[index]);
      }
    }

    #popForeground(status) {
      const child = this.stack.pop();
      child.dispose();
      // Preserve terminal write order: parent bytes before the child's.
      const parentOutput = this.#foreground().takeTerminalOutput();
      if (parentOutput) {
        this.#appendPendingOutput(parentOutput);
      }
      const childOutput = child.takeTerminalOutput();
      if (childOutput) {
        this.#appendPendingOutput(childOutput);
      }
      const depth = this.stack.length;
      this.background = this.background.filter((entry) => {
        if (entry.owner <= depth) return true;
        entry.process.dispose();
        return false;
      });
      this.#foreground().resolveSpawn(status);
    }

    #spawnChild(pkg, argumentsList) {
      if (this.stack.length >= MAX_COMPUTER_PROCESSES) {
        return STATUS_LIMIT;
      }
      const module = this.packages.get(pkg);
      if (module === undefined) {
        return STATUS_NOT_FOUND;
      }
      let context;
      try {
        context = computerContext([pkg, ...argumentsList], this.environment);
      } catch {
        return STATUS_INVALID;
      }
      let child;
      try {
        child = new ComputerProcess(
          module,
          context,
          this.maxGas,
          this.emitLog,
          this.networkProvider,
          this.filesystem,
        );
        child.setTerminalSize(this.columns, this.rows);
        child.setNetworkEnabled(this.network);
      } catch {
        child?.dispose();
        return STATUS_INVALID;
      }
      return child;
    }

    #backgroundIndex(pid) {
      const depth = this.stack.length;
      return this.background.findIndex(
        (entry) => entry.pid === pid && entry.owner === depth,
      );
    }

    #handleChildRequest(request) {
      const resolve = (value) => this.#foreground().resolveSpawn(value);
      if (request.kind.startsWith("workspace")) {
        // Only the root computer holding the workspace grant may manage
        // children; nested computers are never granted it.
        if (!this.workspace || this.stack.length !== 1) {
          resolve(STATUS_DENIED);
          return;
        }
        this.#handleWorkspaceRequest(request, resolve);
        return;
      }
      if (request.kind === "spawn") {
        if (this.background.length >= MAX_BACKGROUND_PROCESSES) {
          resolve(STATUS_LIMIT);
          return;
        }
        const child = this.#spawnChild(request.package, request.arguments);
        if (child === STATUS_NOT_FOUND && this.packageResolution) {
          this.#suspendForPackage(request, "child");
          return;
        }
        if (typeof child === "number") {
          resolve(child);
          return;
        }
        const pid = this.nextPid++;
        this.background.push({
          pid,
          owner: this.stack.length,
          process: child,
          output: [],
          exit: null,
        });
        resolve(pid);
        return;
      }
      const index = this.#backgroundIndex(request.pid);
      if (index < 0) {
        resolve(STATUS_BAD_HANDLE);
        return;
      }
      const entry = this.background[index];
      if (request.kind === "wait") {
        this.#driveBackground(entry);
        if (entry.exit !== null) {
          this.background.splice(index, 1);
          resolve(entry.exit & 0xff);
        } else {
          resolve(STATUS_WOULD_BLOCK);
        }
        return;
      }
      if (request.kind === "pipeWrite") {
        if (entry.exit !== null || entry.process.terminalInputClosed()) {
          resolve(STATUS_INVALID);
          return;
        }
        const space = entry.process.terminalInputSpace();
        const written = Math.min(request.bytes.byteLength, space);
        if (written > 0) {
          entry.process.sendTerminalInput(request.bytes.subarray(0, written));
        }
        this.#driveBackground(entry);
        resolve(written);
        return;
      }
      if (request.kind === "pipeRead") {
        if (entry.output.length === 0) {
          this.#driveBackground(entry);
        }
        if (entry.output.length > 0) {
          const count = Math.min(entry.output.length, request.capacity);
          const bytes = Uint8Array.from(entry.output.slice(0, count));
          entry.output.splice(0, count);
          this.#foreground().resolveRead(request.destination, bytes);
        } else if (entry.exit !== null) {
          resolve(0);
        } else {
          resolve(STATUS_WOULD_BLOCK);
        }
        return;
      }
      // pipeClose
      entry.process.closeTerminalInput();
      this.#driveBackground(entry);
      resolve(0);
    }

    #driveBackground(entry) {
      for (let step = 0; step < MAX_DRIVE_STEPS; step++) {
        if (entry.exit !== null) {
          return;
        }
        let status = null;
        try {
          status = entry.process.run();
        } catch {
          // A faulted piped child fails alone; final output and file
          // writes are still collected below.
        }
        const output = entry.process.takeTerminalOutput();
        if (output) {
          const available = MAX_TTY_OUTPUT_BYTES - entry.output.length;
          const count = Math.min(output.byteLength, Math.max(0, available));
          for (let index = 0; index < count; index++) {
            entry.output.push(output[index]);
          }
        }
        if (status === null) {
          entry.exit = FAULTED_CHILD_STATUS;
          return;
        }
        if (status.kind === "exited") {
          entry.exit = status.code & 0xff;
          return;
        }
        if (status.kind === "yielded") {
          if (!entry.process.hasTerminalInput()) {
            return;
          }
          continue;
        }
        // Background children cannot own the terminal or spawn.
        if (status.kind === "spawn") {
          entry.process.takeSpawnRequest();
        } else {
          entry.process.takeChildRequest();
        }
        entry.process.resolveSpawn(STATUS_DENIED);
      }
    }

    /** Executes one workspace operation and resolves it into the root
     * guest (mirror of handle_workspace_request). */
    #handleWorkspaceRequest(request, resolve) {
      if (request.kind === "workspaceSpawn") {
        if (this.workspaceChildren.length >= MAX_WORKSPACE_CHILDREN) {
          resolve(STATUS_LIMIT);
          return;
        }
        if (this.packageResolution && !this.packages.has(request.package)) {
          this.#suspendForPackage(request, "workspace");
          return;
        }
        resolve(
          this.#spawnWorkspaceChild(
            request.package,
            request.arguments,
            request.columns,
            request.rows,
          ),
        );
        return;
      }
      const index = this.#workspaceIndex(request.handle);
      if (index < 0) {
        resolve(STATUS_BAD_HANDLE);
        return;
      }
      const child = this.workspaceChildren[index];
      if (request.kind === "workspaceSendInput") {
        if (child.exit !== null) {
          resolve(STATUS_INVALID);
          return;
        }
        const space = child.supervisor.terminalInputSpace();
        const written = Math.min(request.bytes.byteLength, space);
        if (written > 0) {
          child.supervisor.sendTerminalInput(
            request.bytes.subarray(0, written),
          );
        }
        this.#driveWorkspaceChild(index);
        resolve(written);
        return;
      }
      if (request.kind === "workspaceRead") {
        if (child.output.length === 0) {
          this.#driveWorkspaceChild(index);
        }
        if (child.output.length > 0) {
          const count = Math.min(child.output.length, request.capacity);
          const bytes = Uint8Array.from(child.output.slice(0, count));
          child.output.splice(0, count);
          this.#foreground().resolveRead(request.destination, bytes);
        } else if (child.exit !== null) {
          resolve(0);
        } else {
          resolve(STATUS_WOULD_BLOCK);
        }
        return;
      }
      if (request.kind === "workspaceResize") {
        if (child.exit !== null) {
          resolve(STATUS_INVALID);
          return;
        }
        try {
          child.supervisor.setTerminalSize(request.columns, request.rows);
        } catch {
          resolve(STATUS_INVALID);
          return;
        }
        resolve(0);
        return;
      }
      if (request.kind === "workspaceWait") {
        this.#driveWorkspaceChild(index);
        // The handle stays valid after exit so remaining output can be
        // drained; workspace_close reclaims the slot.
        resolve(child.exit !== null ? child.exit & 0xff : STATUS_WOULD_BLOCK);
        return;
      }
      // Closing a workspace cancels all of its processes and releases handles.
      child.supervisor.dispose();
      this.workspaceChildren.splice(index, 1);
      resolve(0);
    }

    /** Resolves a workspace child handle to its slot. */
    #workspaceIndex(handle) {
      return this.workspaceChildren.findIndex(
        (child) => child.handle === handle,
      );
    }

    /** Runs a workspace child until it exits or blocks awaiting input,
     * collecting its terminal output; filesystem changes are already shared.
     *
     * Cooperative scheduling: workspace children only execute while the
     * workspace guest is suspended inside a workspace hostcall. */
    #driveWorkspaceChild(index) {
      for (let step = 0; step < MAX_WORKSPACE_DRIVE_STEPS; step++) {
        const child = this.workspaceChildren[index];
        if (child.exit !== null) {
          return;
        }
        // A faulted child fails alone; the workspace observes the fault
        // status through wait. Its final output and writes still land.
        let outcome = null;
        try {
          outcome = child.supervisor.run();
        } catch {
          // Fault: reported below as FAULTED_CHILD_STATUS.
        }
        for (
          let output = child.supervisor.takeTerminalOutput();
          output !== null;
          output = child.supervisor.takeTerminalOutput()
        ) {
          const available = MAX_TTY_OUTPUT_BYTES - child.output.length;
          const count = Math.min(output.byteLength, Math.max(0, available));
          for (let offset = 0; offset < count; offset++) {
            child.output.push(output[offset]);
          }
        }
        let exit = null;
        if (outcome === null) {
          exit = FAULTED_CHILD_STATUS;
        } else if (outcome.kind === "exited") {
          exit = outcome.code & 0xff;
        } else if (outcome.kind === "package") {
          // A suspended package resolution surfaces at the parent's next
          // yielded return; stop driving without progress.
          return;
        } else if (outcome.kind !== "yielded") {
          // A nested supervisor only surfaces yielded, exited, or package.
          exit = FAULTED_CHILD_STATUS;
        }
        if (exit !== null) {
          child.supervisor.dispose();
          child.exit = exit;
          return;
        }
        if (!child.supervisor.hasTerminalInput()) {
          return;
        }
      }
    }

    /** Launches an independently supervised workspace child: a complete
     * nested computer whose terminal endpoint is the parent-held handle. */
    #spawnWorkspaceChild(pkg, argumentsList, columns, rows) {
      if (columns < 1 || columns > 1000 || rows < 1 || rows > 1000) {
        return STATUS_INVALID;
      }
      const module = this.packages.get(pkg);
      if (module === undefined) {
        return STATUS_NOT_FOUND;
      }
      let child;
      try {
        const context = computerContext(
          [pkg, ...argumentsList],
          this.environment,
        );
        child = new ComputerSupervisor(
          module,
          context,
          this.maxGas,
          this.emitLog,
          {
            networkProvider: this.networkProvider,
            packageResolution: this.packageResolution,
            filesystem: this.filesystem,
          },
        );
        // The nested computer shares the Host-authorized package registry
        // so a shell pane can run editors; it is never granted
        // host.workspace.
        child.packages = new Map(this.packages);
        child.setTerminalSize(columns, rows);
        child.setNetworkEnabled(this.network);
      } catch {
        child?.dispose();
        return STATUS_INVALID;
      }
      const handle = this.nextPid++;
      this.workspaceChildren.push({
        handle,
        supervisor: child,
        output: [],
        exit: null,
      });
      return handle;
    }
  }

  /** Translates a `.polkavm` program to a compiled program through the staged
   * translator in polkavm-browser-runtime.wasm. */
  class ComputerTranslator {
    constructor(runtimeExports) {
      this.pvm = runtimeExports;
    }

    static async create(runtimeWasm) {
      const imports = {
        polkavm_browser: {
          clock_wall_ms: () => 0,
          random_fill: () => STATUS_DENIED,
        },
      };
      const { instance } =
        runtimeWasm instanceof WebAssembly.Module
          ? { instance: new WebAssembly.Instance(runtimeWasm, imports) }
          : await WebAssembly.instantiate(runtimeWasm, imports);
      const exports = instance.exports;
      if (exports.polkavm_browser_abi_version() !== 2) {
        throw new Error("PolkaVM browser runtime ABI mismatch");
      }
      return new ComputerTranslator(exports);
    }

    #errorText() {
      const pointer = this.pvm.polkavm_browser_error_pointer();
      const length = this.pvm.polkavm_browser_error_length();
      return new TextDecoder().decode(
        new Uint8Array(this.pvm.memory.buffer, pointer, length),
      );
    }

    #translateBytes(source, partitioned) {
      const pointer = this.pvm.polkavm_browser_staging_reserve(source.byteLength);
      if (!pointer) {
        throw new Error(`reserve staging memory: ${this.#errorText()}`);
      }
      new Uint8Array(this.pvm.memory.buffer, pointer, source.byteLength).set(
        source,
      );
      const status = partitioned
        ? this.pvm.polkavm_browser_translate_partitioned_staged()
        : this.pvm.polkavm_browser_translate_staged();
      if (status !== 0) {
        throw new Error(`translate computer guest: ${this.#errorText()}`);
      }
      const output = this.pvm.polkavm_browser_translation_pointer();
      const length = this.pvm.polkavm_browser_translation_length();
      return new Uint8Array(
        this.pvm.memory.buffer,
        output,
        length,
      ).slice();
    }

    async translate(programBytes) {
      const source =
        programBytes instanceof Uint8Array
          ? programBytes
          : new Uint8Array(programBytes);
      const bytes = this.#translateBytes(source, false);
      let module;
      try {
        module = await WebAssembly.compile(bytes);
      } catch (error) {
        if (error instanceof WebAssembly.CompileError) {
          throw error;
        }
        module = await WebAssembly.compile(this.#translateBytes(source, true));
      }
      const parts = [];
      for (const part of WebAssembly.Module.customSections(
        module,
        "epoca.pvm.code-part",
      )) {
        parts.push(await WebAssembly.compile(part));
      }
      return { module, parts };
    }
  }

  globalThis.PolkaVmComputer = {
    computerContext,
    ComputerDevices,
    ComputerProcess,
    ComputerSupervisor,
    ComputerTranslator,
    WebSocketTcpProvider,
    STATUS_WOULD_BLOCK,
    STATUS_BAD_HANDLE,
    STATUS_INVALID,
    STATUS_NOT_FOUND,
    STATUS_DENIED,
    STATUS_LIMIT,
    STATUS_EXISTS,
    STATUS_NOT_DIRECTORY,
    STATUS_IS_DIRECTORY,
    STATUS_NOT_EMPTY,
    FS_OPEN_READ,
    FS_OPEN_WRITE,
    FS_OPEN_CREATE,
    FS_OPEN_TRUNCATE,
    FS_OPEN_EXCLUSIVE,
    FS_OPEN_APPEND,
    TTY_MODE_RAW,
    TTY_MODE_ECHO,
  };
})();

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

(() => {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const PREFIX = encoder.encode("PVMFS001");
  const MAX_FRAME_BYTES = 1024 * 1024;
  const MAX_REQUESTS = 32;
  const MAX_QUEUED_BYTES = 4 * 1024 * 1024;
  const CHUNK_BYTES = 262144;
  const LIMITS = Object.freeze({
    maxFiles: 64,
    maxDirectories: 256, // In addition to the fixed /home root.
    maxFileBytes: 1048576,
    maxPathBytes: 200,
  });
  const EMPTY = new Uint8Array();
  const C = globalThis.PolkaVmComputer;

  function isWorkspaceFrame(bytes) {
    return (
      bytes instanceof Uint8Array &&
      bytes.length >= PREFIX.length &&
      PREFIX.every((byte, index) => bytes[index] === byte)
    );
  }

  function decodeFrame(bytes) {
    if (
      !isWorkspaceFrame(bytes) ||
      bytes.length < 12 ||
      bytes.length > MAX_FRAME_BYTES
    ) {
      throw new Error("invalid workspace frame envelope");
    }
    const length = new DataView(
      bytes.buffer,
      bytes.byteOffset,
      bytes.byteLength,
    ).getUint32(8, true);
    if (length === 0 || length > bytes.length - 12) {
      throw new Error("truncated workspace frame header");
    }
    const header = JSON.parse(decoder.decode(bytes.subarray(12, 12 + length)));
    if (
      header === null ||
      typeof header !== "object" ||
      Array.isArray(header)
    ) {
      throw new Error("invalid workspace frame header");
    }
    return { header, payload: bytes.subarray(12 + length) };
  }

  function encodeFrame(header, payload = EMPTY) {
    const json = encoder.encode(JSON.stringify(header));
    if (
      !(payload instanceof Uint8Array) ||
      12 + json.length + payload.length > MAX_FRAME_BYTES
    ) {
      throw new Error("workspace frame exceeds the host limit");
    }
    const bytes = new Uint8Array(12 + json.length + payload.length);
    bytes.set(PREFIX);
    new DataView(bytes.buffer).setUint32(8, json.length, true);
    bytes.set(json, 12);
    bytes.set(payload, 12 + json.length);
    return bytes;
  }

  function fail(status, message) {
    const error = new Error(message);
    error.status = status;
    throw error;
  }

  function check(status, operation) {
    if (status < 0) fail(status, `${operation} failed (${status})`);
    return status;
  }

  function keys(value, allowed) {
    return (
      value !== null &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      Object.keys(value).every((key) => allowed.includes(key))
    );
  }

  function integer(value, max) {
    return Number.isInteger(value) && value >= 0 && value <= max;
  }

  function decimal(value) {
    return (
      typeof value === "string" &&
      /^(0|[1-9][0-9]{0,19})$/.test(value) &&
      BigInt(value) <= 0xffffffffffffffffn
    );
  }

  const OPERATIONS = {
    workspace: [],
    metadata: ["path"],
    list: ["path"],
    read: ["path", "offset", "capacity", "inode", "mtimeNs"],
    begin_write: ["path", "length", "overwrite", "ignoreIfExists"],
    write_chunk: ["offset"],
    commit_write: [],
    mkdir: ["path"],
    rename: ["path", "to", "overwrite", "ignoreIfExists", "createParents"],
    remove: ["path", "kind", "recursive", "ignoreIfNotExists"],
    copy: ["path", "to", "overwrite", "ignoreIfExists"],
    readable_copy: ["path"],
  };

  /** A private Computer namespace. respond resolves only after the runtime has
   * accepted the frame; commit resolves only after the Host's durable transaction.
   * Neither hook, nor a project identifier, is accessible through guest frames. */
  class WorkspaceService {
    #devices = null;
    #info;
    #commit;
    #format;
    #respond;
    #fatal;
    #queue = [];
    #ids = new Set();
    #queuedBytes = 0;
    #busy = false;
    #closed = false;
    #upload = null;
    #rollback = null;
    #cancelCommit = null;

    constructor(snapshot, { commit, format, respond, fatal }) {
      if (
        !C ||
        typeof respond !== "function" ||
        typeof fatal !== "function" ||
        typeof commit !== "function"
      ) {
        throw new Error(
          "workspace service requires ComputerDevices and Host callbacks",
        );
      }
      this.#commit = commit;
      this.#format = format;
      this.#respond = respond;
      this.#fatal = fatal;
      if (snapshot == null) return;
      if (
        !keys(snapshot, ["name", "files", "metadata"]) ||
        typeof snapshot.name !== "string" ||
        decoder.decode(encoder.encode(snapshot.name)) !== snapshot.name ||
        !Array.isArray(snapshot.files) ||
        snapshot.files.length > LIMITS.maxFiles
      ) {
        throw new Error("invalid workspace snapshot");
      }
      const metadata = snapshot.metadata;
      if (
        !metadata ||
        !decimal(metadata.nextInode) ||
        !decimal(metadata.clockNs) ||
        !Array.isArray(metadata.entries) ||
        metadata.entries.length > LIMITS.maxFiles + LIMITS.maxDirectories + 1 ||
        metadata.entries.some(
          (entry) => !entry || !decimal(entry.inode) || !decimal(entry.mtimeNs),
        )
      ) {
        throw new Error("invalid workspace filesystem metadata");
      }
      this.#info = encoder.encode(
        JSON.stringify({ root: "/home", name: snapshot.name, limits: LIMITS }),
      );
      // Check the complete reply bound before any guest initialization.
      encodeFrame({ id: 0xffffffff, status: 0 }, this.#info);
      const devices = new C.ComputerDevices();
      try {
        const paths = new Set();
        for (const file of snapshot.files) {
          if (
            !keys(file, ["path", "bytes"]) ||
            typeof file.path !== "string" ||
            !(file.bytes instanceof Uint8Array) ||
            paths.has(file.path)
          ) {
            throw new Error("invalid or duplicate workspace file");
          }
          paths.add(file.path);
          devices.mountFile(file.path, file.bytes);
        }
        // Import enforces exact byte/metadata correspondence, unique inodes,
        // canonical paths, explicit parent directories and version-1 counters.
        devices.importFilesystemMetadata(snapshot.metadata);
        this.#devices = devices;
      } catch (error) {
        devices.dispose();
        throw error;
      }
    }

    /** Retains ownership of bytes until the bounded serialized request finishes.
     * Uncorrelatable frames/duplicate active IDs are fatal rather than hanging. */
    receive(bytes) {
      if (this.#closed) throw new Error("workspace service is stopped");
      const request = decodeFrame(bytes);
      const { id } = request.header;
      if (!integer(id, 0xffffffff) || id === 0 || this.#ids.has(id)) {
        throw new Error("invalid or duplicate workspace request id");
      }
      if (
        this.#ids.size >= MAX_REQUESTS ||
        this.#queuedBytes + bytes.length > MAX_QUEUED_BYTES
      ) {
        throw new Error("workspace request queue limit exceeded");
      }
      this.#ids.add(id);
      this.#queuedBytes += bytes.length;
      this.#queue.push({ ...request, length: bytes.length });
      void this.#drain();
    }

    stop() {
      if (this.#closed) return;
      this.#closed = true;
      this.#upload = null;
      this.#queue.length = 0;
      this.#ids.clear();
      this.#queuedBytes = 0;
      this.#cancelCommit?.(
        new Error("workspace service stopped before commit acknowledgement"),
      );
      this.#restore();
      this.#devices?.dispose();
    }

    async #drain() {
      if (this.#busy) return;
      this.#busy = true;
      try {
        while (!this.#closed && this.#queue.length) {
          const { header, payload, length } = this.#queue.shift();
          let result;
          try {
            this.#validate(header, payload);
            if (!this.#devices)
              fail(C.STATUS_DENIED, "workspace capability is not enabled");
            result = await this.#execute(header, payload);
          } catch (error) {
            if (
              ["begin_write", "write_chunk", "commit_write"].includes(header.op)
            ) {
              this.#upload = null;
            }
            result = {
              status: Number.isInteger(error.status) ? error.status : -1,
              error: String(error.message ?? error).slice(0, 2048),
            };
          }
          if (this.#closed) break;
          const { payload: responsePayload, ...response } = result;
          await this.#respond(
            encodeFrame({ id: header.id, ...response }, responsePayload),
          );
          if (this.#closed) break;
          this.#ids.delete(header.id);
          this.#queuedBytes -= length;
        }
      } catch (error) {
        this.stop();
        this.#fatal(error);
      } finally {
        this.#busy = false;
      }
    }

    #validate(header, payload) {
      const fields =
        typeof header.op === "string" && Object.hasOwn(OPERATIONS, header.op)
          ? OPERATIONS[header.op]
          : null;
      if (!fields || !keys(header, ["id", "op", ...fields])) {
        fail(C.STATUS_INVALID, "invalid workspace operation or arguments");
      }
      if (header.op !== "write_chunk" && payload.length !== 0) {
        fail(C.STATUS_INVALID, "unexpected workspace request payload");
      }
      for (const name of fields) {
        const value = header[name];
        if (name === "inode" || name === "mtimeNs") {
          if (value !== undefined && !decimal(value))
            fail(C.STATUS_INVALID, `invalid ${name}`);
        } else if (name === "path" || name === "to") {
          if (typeof value !== "string")
            fail(C.STATUS_INVALID, `invalid ${name}`);
          // Reuse Computer's path rules even for recursive operations below.
          if (this.#devices?.fsMetadata(value).status === C.STATUS_INVALID) {
            fail(C.STATUS_INVALID, `invalid workspace ${name}`);
          }
        } else if (
          [
            "overwrite",
            "ignoreIfExists",
            "createParents",
            "recursive",
            "ignoreIfNotExists",
          ].includes(name)
        ) {
          if (typeof value !== "boolean")
            fail(C.STATUS_INVALID, `invalid ${name}`);
        } else if (name === "kind") {
          if (value !== 1 && value !== 2)
            fail(C.STATUS_INVALID, "invalid remove kind");
        } else if (
          !integer(
            value,
            name === "capacity" ? CHUNK_BYTES : LIMITS.maxFileBytes,
          )
        ) {
          fail(C.STATUS_INVALID, `invalid ${name}`);
        }
      }
      if (payload.length > CHUNK_BYTES)
        fail(C.STATUS_LIMIT, "workspace chunk exceeds limit");
    }

    #entry(path) {
      const result = this.#devices.fsMetadata(path);
      check(result.status, `metadata ${path}`);
      const view = new DataView(result.record.buffer);
      return {
        kind: view.getUint32(0, true),
        inode: view.getBigUint64(16, true),
        mtimeNs: view.getBigUint64(8, true),
      };
    }

    #destination(path, overwrite, ignoreIfExists) {
      const result = this.#devices.fsMetadata(path);
      if (result.status === C.STATUS_NOT_FOUND) return false;
      check(result.status, `metadata ${path}`);
      if (overwrite) return false;
      if (ignoreIfExists) return true;
      fail(C.STATUS_EXISTS, `workspace destination exists: ${path}`);
    }

    #prepareWrite(path, overwrite, ignoreIfExists) {
      if (this.#destination(path, overwrite, ignoreIfExists)) return true;
      const fs = this.#devices.filesystem;
      if (fs.entries.get(path)?.kind === 2)
        fail(C.STATUS_IS_DIRECTORY, `${path} is a directory`);
      check(fs.parentStatus(path), `parent of ${path}`);
      if (!fs.entries.has(path) && fs.files.size >= LIMITS.maxFiles) {
        fail(C.STATUS_LIMIT, "workspace file limit exceeded");
      }
      if (!fs.canMutate(fs.entries.has(path) ? 0 : 1)) {
        fail(C.STATUS_LIMIT, "workspace metadata limit exceeded");
      }
      return false;
    }

    #readablePath(path) {
      const slash = path.lastIndexOf("/");
      const dot = path.lastIndexOf(".");
      const extension = dot > slash + 1 ? path.slice(dot) : "";
      const stem = extension ? path.slice(0, dot) : path;
      for (let suffix = 1; ; suffix++) {
        const target = `${stem}.readable${suffix === 1 ? "" : `-${suffix}`}${extension}`;
        if (encoder.encode(target).length > LIMITS.maxPathBytes)
          fail(
            C.STATUS_LIMIT,
            "readable copy path exceeds workspace path limit",
          );
        const result = this.#devices.fsMetadata(target);
        if (result.status === C.STATUS_NOT_FOUND) return target;
        check(result.status, `metadata ${target}`);
      }
    }

    #replaceFile(path, bytes) {
      const handle = check(
        this.#devices.fsOpen(
          path,
          C.FS_OPEN_WRITE | C.FS_OPEN_CREATE | C.FS_OPEN_TRUNCATE,
        ),
        `open ${path}`,
      );
      try {
        // TRUNCATE replaces the old buffer before fsWrite. Rollback references
        // therefore cannot be modified even for shorter or same-length writes.
        const written = check(
          this.#devices.fsWrite(handle, bytes),
          `write ${path}`,
        );
        if (written !== bytes.length)
          fail(-1, `incomplete workspace write: ${path}`);
      } finally {
        this.#devices.fsClose(handle);
      }
    }

    #mkdir(path) {
      let current = "/home";
      let created = false;
      for (const part of path.slice(6).split("/")) {
        if (!part) continue;
        current += `/${part}`;
        const result = this.#devices.fsMetadata(current);
        if (result.status === C.STATUS_NOT_FOUND) {
          check(this.#devices.fsMkdir(current), `mkdir ${current}`);
          created = true;
        } else {
          check(result.status, `metadata ${current}`);
          if (new DataView(result.record.buffer).getUint32(0, true) !== 2) {
            fail(C.STATUS_NOT_DIRECTORY, `${current} is not a directory`);
          }
        }
      }
      return created ? { status: 0 } : { status: 0, ignored: true };
    }

    #restore() {
      if (!this.#rollback) return;
      const { files, metadata } = this.#rollback;
      for (const handle of this.#devices.openFiles.keys())
        this.#devices.fsClose(handle);
      this.#devices.files.clear();
      for (const [path, bytes] of files) this.#devices.files.set(path, bytes);
      this.#devices.modified.clear();
      this.#devices.removed.clear();
      this.#devices.importFilesystemMetadata(metadata);
      this.#rollback = null;
    }

    async #mutate(action) {
      this.#rollback = {
        metadata: this.#devices.exportFilesystemMetadata(),
        files: new Map(this.#devices.files),
      };
      try {
        const result = action();
        const metadata = this.#devices.takeFilesystemMetadata();
        if (metadata !== null) {
          // takeModifiedFiles returns copies; transferring them cannot detach
          // live filesystem data or the old buffers retained for rollback.
          const modified = this.#devices
            .takeModifiedFiles()
            .map(([path, bytes]) => ({ path, bytes }));
          const removed = this.#devices.takeRemovedFiles();
          const cancelled = new Promise((_, reject) => {
            this.#cancelCommit = reject;
          });
          await Promise.race([
            cancelled,
            this.#commit({ modified, removed, metadata }),
          ]);
          if (this.#closed) throw new Error("workspace service stopped");
        }
        this.#rollback = null;
        return result ?? { status: 0 };
      } catch (error) {
        this.#restore();
        throw error;
      } finally {
        this.#cancelCommit = null;
      }
    }

    async #execute(request, payload) {
      const { op, path } = request;
      switch (op) {
        case "workspace":
          return { status: 0, payload: this.#info };
        case "metadata": {
          const result = this.#devices.fsMetadata(path);
          check(result.status, `metadata ${path}`);
          return { status: result.status, payload: result.record };
        }
        case "list": {
          const result = this.#devices.fsListDirectory(path);
          check(result.status, `list ${path}`);
          return { status: result.status, payload: result.record };
        }
        case "read": {
          const entry = this.#entry(path);
          if (
            (request.inode !== undefined &&
              entry.inode !== BigInt(request.inode)) ||
            (request.mtimeNs !== undefined &&
              entry.mtimeNs !== BigInt(request.mtimeNs))
          ) {
            fail(
              C.STATUS_DENIED,
              "workspace file identity or version changed during read",
            );
          }
          const handle = check(
            this.#devices.fsOpen(path, C.FS_OPEN_READ),
            `open ${path}`,
          );
          try {
            check(
              this.#devices.fsSeek(handle, request.offset, 0),
              `seek ${path}`,
            );
            const result = this.#devices.fsRead(handle, request.capacity);
            check(result.status, `read ${path}`);
            return { status: result.status, payload: result.bytes };
          } finally {
            this.#devices.fsClose(handle);
          }
        }
        case "readable_copy": {
          if (typeof this.#format !== "function")
            fail(
              C.STATUS_DENIED,
              "readable copy formatting is not supported by this Host",
            );
          if (this.#entry(path).kind !== 1)
            fail(C.STATUS_IS_DIRECTORY, `${path} is a directory`);
          const target = this.#readablePath(path);
          this.#prepareWrite(target, false, false);
          // The formatter owns this copy and may transfer its buffer. It never
          // receives the saved filesystem buffer or any guest editor contents.
          const bytes = await this.#format(
            path,
            this.#devices.files.get(path).slice(),
          );
          if (this.#closed) throw new Error("workspace service stopped");
          if (!(bytes instanceof Uint8Array))
            fail(C.STATUS_INVALID, "readable formatter returned invalid bytes");
          if (bytes.length > LIMITS.maxFileBytes)
            fail(
              C.STATUS_LIMIT,
              "readable copy exceeds workspace file size limit",
            );
          return this.#mutate(() => {
            this.#replaceFile(target, bytes);
            return { status: 0, payload: encoder.encode(target) };
          });
        }
        case "begin_write":
          this.#upload = null;
          if (
            this.#prepareWrite(path, request.overwrite, request.ignoreIfExists)
          ) {
            return { status: 0, ignored: true };
          }
          this.#upload = {
            path,
            bytes: new Uint8Array(request.length),
            offset: 0,
            overwrite: request.overwrite,
            ignoreIfExists: request.ignoreIfExists,
          };
          return { status: 0 };
        case "write_chunk": {
          const upload = this.#upload;
          if (
            !upload ||
            request.offset !== upload.offset ||
            payload.length === 0 ||
            upload.offset + payload.length > upload.bytes.length
          ) {
            fail(
              C.STATUS_INVALID,
              "workspace write chunk is missing, noncontiguous or exceeds staged length",
            );
          }
          upload.bytes.set(payload, upload.offset);
          upload.offset += payload.length;
          return { status: 0 };
        }
        case "commit_write": {
          const upload = this.#upload;
          this.#upload = null;
          if (!upload || upload.offset !== upload.bytes.length) {
            fail(C.STATUS_INVALID, "workspace upload is incomplete");
          }
          return this.#mutate(() => {
            if (
              this.#prepareWrite(
                upload.path,
                upload.overwrite,
                upload.ignoreIfExists,
              )
            ) {
              return { status: 0, ignored: true };
            }
            this.#replaceFile(upload.path, upload.bytes);
          });
        }
        case "mkdir":
          return this.#mutate(() => this.#mkdir(path));
        case "rename":
          return this.#mutate(() => {
            this.#entry(path);
            if (path === request.to) return { status: 0, ignored: true };
            if (
              this.#destination(
                request.to,
                request.overwrite,
                request.ignoreIfExists,
              )
            ) {
              return { status: 0, ignored: true };
            }
            if (request.createParents)
              this.#mkdir(request.to.slice(0, request.to.lastIndexOf("/")));
            check(this.#devices.fsRename(path, request.to), `rename ${path}`);
          });
        case "remove":
          return this.#mutate(() => {
            const metadata = this.#devices.fsMetadata(path);
            if (
              metadata.status === C.STATUS_NOT_FOUND &&
              request.ignoreIfNotExists
            ) {
              return { status: 0, ignored: true };
            }
            check(metadata.status, `metadata ${path}`);
            const kind = new DataView(metadata.record.buffer).getUint32(
              0,
              true,
            );
            if (kind !== request.kind) {
              fail(
                kind === 2 ? C.STATUS_IS_DIRECTORY : C.STATUS_NOT_DIRECTORY,
                "workspace remove kind mismatch",
              );
            }
            if (path === "/home")
              fail(C.STATUS_DENIED, "cannot remove workspace root");
            if (kind === 2 && request.recursive) {
              const children = [...this.#devices.filesystem.entries.keys()]
                .filter((child) => child.startsWith(`${path}/`))
                .sort((a, b) => b.length - a.length);
              for (const child of children) {
                const childKind =
                  this.#devices.filesystem.entries.get(child).kind;
                check(
                  childKind === 1
                    ? this.#devices.fsRemove(child)
                    : this.#devices.fsRmdir(child),
                  `remove ${child}`,
                );
              }
            }
            check(
              kind === 1
                ? this.#devices.fsRemove(path)
                : this.#devices.fsRmdir(path),
              `remove ${path}`,
            );
          });
        case "copy":
          return this.#mutate(() => {
            if (this.#entry(path).kind !== 1)
              fail(C.STATUS_IS_DIRECTORY, `${path} is a directory`);
            if (
              this.#prepareWrite(
                request.to,
                request.overwrite,
                request.ignoreIfExists,
              )
            ) {
              return { status: 0, ignored: true };
            }
            this.#replaceFile(request.to, this.#devices.files.get(path));
          });
      }
    }
  }

  globalThis.PolkaVmWorkspace = {
    WorkspaceService,
    isWorkspaceFrame,
    decodeFrame,
    encodeFrame,
  };
})();

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Creates the bounded PolkaVM endpoint in a Worker or on a host thread.
 *
 * Load polkavm-computer.js and polkavm-workspace.js before this source. A Host
 * grants an origin-private namespace through start.workspace:
 * {name, files: [{path, bytes: Uint8Array}], metadata: FilesystemMetadata}.
 * The endpoint receives workspace-commit {id, modified, removed, metadata}
 * and must send workspace-commit-result {id, ok, error?} only after its durable
 * transaction completes. IDs and storage selection are Host-owned; they never
 * appear in guest filesystem requests. Missing workspace grants return DENIED.
 * This acknowledgement is independent of Computer fs_sync (visibility only).
 * Hosts that set start.workspaceFormat=true also receive workspace-format
 * {id, path, bytes: Uint8Array} and return workspace-format-result
 * {id, ok:true, bytes: Uint8Array} or {id, ok:false, error:string}. Format IDs
 * are independent of commit IDs; formatting alone never persists a file.
 *
 * @param {DedicatedWorkerGlobalScope} endpoint - Message endpoint owned by the runtime.
 */
globalThis.createPolkaVmRuntime = (endpoint) => {
  const postMessage = (message, transfers) => {
    if (transfers) {
      endpoint.postMessage(message, transfers);
    } else {
      endpoint.postMessage(message);
    }
  };

  const LEGACY_FRAME_INTERVAL_MS = 1000 / 60;
  const MAX_GAS_PER_UPDATE = 10_000_000_000;
  const MAX_TRANSLATED_LOOPS_PER_UPDATE = 50_000_000;
  const MAX_PROGRAM_BYTES = 128 * 1024 * 1024;
  const MAX_ASSET_FILES = 2048;
  const MAX_ASSET_NAME_BYTES = 1024;
  const MAX_ASSET_FILE_BYTES = 128 * 1024 * 1024;
  const MAX_ASSET_BYTES = 256 * 1024 * 1024;
  const MOTION_SAMPLE_BYTES = 48;
  const MAX_MEDIATED_INPUT_KIND_BYTES = 32;
  const MAX_MEDIATED_INPUT_REGISTRATIONS = 8;
  const RANDOM_BYTES_PER_EXECUTION = 64 * 1024;
  // Safe-area (16) and virtual-keyboard (17) inset records. Both records of one
  // update carry a single axis, so a Host sends them through the dedicated
  // `view-insets` message that queues the pair together; the runtime rejects a
  // lone axis arriving as an ordinary input record.
  const INPUT_SAFE_AREA_INSETS = 16;
  const INPUT_KEYBOARD_INSETS = 17;
  const MAX_INSET_PIXELS = 65535;
  const UPDATE_AFTER_IDLE = 0xffffffff;
  const FORCE_INTERPRETER = Symbol("force-interpreter");
  const CORE_STATUS_DENIED = -5;
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();

  function executionRandomBytes() {
    const bytes = new Uint8Array(RANDOM_BYTES_PER_EXECUTION);
    const browserCrypto = globalThis.crypto;
    if (typeof browserCrypto?.getRandomValues !== "function") {
      return new Uint8Array();
    }
    try {
      browserCrypto.getRandomValues(bytes);
      return bytes;
    } catch {
      return new Uint8Array();
    }
  }

  let pvm;
  let translated;
  let backend = "interpreter";
  let running = false;
  let disposed = false;
  let starting = false;
  let workspace = null;
  let workspaceCommit = null;
  let nextWorkspaceCommitId = 1;
  let workspaceFormat = null;
  let nextWorkspaceFormatId = 1;
  const workspaceResponses = [];
  let workspaceResponseBytes = 0;
  let paused = false;
  let pausedAt = 0;
  let demandDriven = false;
  let tickPending = false;
  let motionAvailability = 0;
  let pendingMotionSample = null;
  let pointerCaptureSupported = false;
  let pendingGpuCapabilities = null;
  let timer;
  let startedAt = 0;
  let updateCount = 0;
  const updateSamples = [];
  const activeMediatedInputHandles = new Set();
  const heldInputs = new Map();
  const tickChannel = new MessageChannel();
  tickChannel.port1.onmessage = () => {
    tickPending = false;
    tick();
  };

  function stopRuntime() {
    if (disposed) {
      return;
    }
    disposed = true;
    for (const handle of activeMediatedInputHandles) {
      postMessage({ type: "mediated-input-cancel", handle });
    }
    activeMediatedInputHandles.clear();
    running = false;
    workspace?.stop();
    workspaceCommit?.reject(new Error("PolkaVM workspace stopped"));
    workspaceCommit = null;
    workspaceFormat?.reject(new Error("PolkaVM workspace stopped"));
    workspaceFormat = null;
    for (const response of workspaceResponses) {
      response.reject(new Error("PolkaVM workspace stopped"));
    }
    workspaceResponses.length = 0;
    workspaceResponseBytes = 0;
    heldInputs.clear();
    clearTimeout(timer);
    translated?.stop();
    pvm?.polkavm_browser_reset?.();
    tickChannel.port1.close();
    tickChannel.port2.close();
    endpoint.onmessage = null;
  }

  function postRuntimeOutput(output, transfers = []) {
    if (
      output?.type === "host-frame-request" &&
      routeWorkspaceFrame(output.bytes)
    ) {
      return;
    }
    // The Host also clears already delivered audio when it requests a pause.
    if (paused && output?.type === "audio") {
      return;
    }
    if (output?.type === "mediated-input-request") {
      activeMediatedInputHandles.add(output.handle);
    } else if (output?.type === "mediated-input-cancel") {
      activeMediatedInputHandles.delete(output.handle);
    }
    postMessage(output, transfers);
  }

  function errorText() {
    const pointer = pvm.polkavm_browser_error_pointer();
    const length = pvm.polkavm_browser_error_length();
    return decoder.decode(new Uint8Array(pvm.memory.buffer, pointer, length));
  }

  function check(status, operation) {
    if (status !== 0) {
      throw new Error(`${operation}: ${errorText()}`);
    }
  }

  function stage(bytes) {
    const source =
      bytes instanceof Uint8Array
        ? bytes
        : new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const pointer = pvm.polkavm_browser_staging_reserve(source.byteLength);
    if (!pointer) {
      throw new Error(`reserve browser runtime memory: ${errorText()}`);
    }
    new Uint8Array(pvm.memory.buffer, pointer, source.byteLength).set(source);
  }

  function addAsset(asset) {
    const path = encoder.encode(asset.path.replace(/^\/+/, ""));
    const bytes = new Uint8Array(asset.bytes);
    const packed = new Uint8Array(path.byteLength + bytes.byteLength);
    packed.set(path);
    packed.set(bytes, path.byteLength);
    stage(packed);
    check(
      pvm.polkavm_browser_launch_add_asset(path.byteLength),
      `mount browser asset ${asset.path}`,
    );
  }

  function drainFrame() {
    if (!pvm.polkavm_browser_take_frame()) {
      return;
    }
    const width = pvm.polkavm_browser_frame_width();
    const height = pvm.polkavm_browser_frame_height();
    const length = pvm.polkavm_browser_frame_length();
    const source = new Uint8Array(
      pvm.memory.buffer,
      pvm.polkavm_browser_frame_pointer(),
      length,
    );
    const pixels = new Uint8Array(length);
    for (let index = 0; index < length; index += 4) {
      pixels[index] = source[index + 2];
      pixels[index + 1] = source[index + 1];
      pixels[index + 2] = source[index];
      pixels[index + 3] = source[index + 3];
    }
    postMessage({ type: "frame", width, height, pixels }, [pixels.buffer]);
  }

  function drainTri2d() {
    if (!pvm.polkavm_browser_take_tri2d?.()) {
      return;
    }
    const length = pvm.polkavm_browser_tri2d_length();
    const bytes = new Uint8Array(
      pvm.memory.buffer,
      pvm.polkavm_browser_tri2d_pointer(),
      length,
    ).slice();
    postMessage({ type: "tri2d", bytes }, [bytes.buffer]);
  }

  function drainUiSemantics() {
    if (!pvm.polkavm_browser_take_ui_semantics?.()) {
      return;
    }
    const length = pvm.polkavm_browser_ui_semantics_length();
    const bytes = new Uint8Array(
      pvm.memory.buffer,
      pvm.polkavm_browser_ui_semantics_pointer(),
      length,
    ).slice();
    postMessage({ type: "ui-semantics", bytes }, [bytes.buffer]);
  }

  function drainUiOutput() {
    if (!pvm.polkavm_browser_take_ui_output?.()) {
      return;
    }
    const length = pvm.polkavm_browser_ui_output_length();
    const bytes = new Uint8Array(
      pvm.memory.buffer,
      pvm.polkavm_browser_ui_output_pointer(),
      length,
    ).slice();
    const output = globalThis.decodePolkaVmUiOutput?.(bytes);
    if (output == null) {
      throw new Error("interpreter emitted invalid UI output");
    }
    postMessage({ type: "ui-output", output });
  }

  function drainGpuBatches() {
    while (pvm.polkavm_browser_take_gpu_batch?.()) {
      const length = pvm.polkavm_browser_gpu_batch_length();
      const bytes = new Uint8Array(
        pvm.memory.buffer,
        pvm.polkavm_browser_gpu_batch_pointer(),
        length,
      ).slice();
      postMessage({ type: "gpu-batch", bytes }, [bytes.buffer]);
    }
  }

  function drainHostFrameRequests() {
    while (pvm.polkavm_browser_take_host_frame_request?.()) {
      const length = pvm.polkavm_browser_host_frame_request_length();
      const bytes = new Uint8Array(
        pvm.memory.buffer,
        pvm.polkavm_browser_host_frame_request_pointer(),
        length,
      ).slice();
      if (!routeWorkspaceFrame(bytes)) {
        postMessage({ type: "host-frame-request", bytes }, [bytes.buffer]);
      }
    }
  }

  function routeWorkspaceFrame(bytes) {
    if (!globalThis.PolkaVmWorkspace?.isWorkspaceFrame(bytes)) return false;
    if (!workspace) {
      throw new Error("PolkaVM workspace service is unavailable");
    }
    workspace.receive(bytes);
    return true;
  }

  function commitWorkspace(delta) {
    return new Promise((resolve, reject) => {
      if (
        disposed ||
        workspaceCommit !== null ||
        !Number.isSafeInteger(nextWorkspaceCommitId)
      ) {
        reject(new Error("PolkaVM workspace commit is unavailable"));
        return;
      }
      const id = nextWorkspaceCommitId++;
      workspaceCommit = { id, resolve, reject };
      try {
        // Service deltas contain copies, never live Computer filesystem buffers.
        postMessage(
          { type: "workspace-commit", id, ...delta },
          delta.modified.map((file) => file.bytes.buffer),
        );
      } catch (error) {
        workspaceCommit = null;
        reject(error);
      }
    });
  }

  function receiveWorkspaceCommit(message) {
    // Only the Host endpoint reaches this function. Guest host frames cannot
    // acknowledge persistence. Unknown/stale IDs never settle another commit.
    const pending = workspaceCommit;
    if (!pending || message.id !== pending.id) return;
    workspaceCommit = null;
    if (
      typeof message.ok !== "boolean" ||
      (message.error !== undefined && typeof message.error !== "string")
    ) {
      pending.reject(
        new Error("invalid workspace persistence acknowledgement"),
      );
    } else if (message.ok) {
      pending.resolve();
    } else {
      pending.reject(
        new Error(message.error || "workspace persistence failed"),
      );
    }
  }

  function formatWorkspace(path, bytes) {
    return new Promise((resolve, reject) => {
      if (
        disposed ||
        workspaceFormat !== null ||
        !Number.isSafeInteger(nextWorkspaceFormatId)
      ) {
        reject(new Error("PolkaVM workspace formatter is unavailable"));
        return;
      }
      const id = nextWorkspaceFormatId++;
      workspaceFormat = { id, resolve, reject };
      try {
        // WorkspaceService supplies an owned copy, not its saved file buffer.
        postMessage({ type: "workspace-format", id, path, bytes }, [
          bytes.buffer,
        ]);
      } catch (error) {
        workspaceFormat = null;
        reject(error);
      }
    });
  }

  function receiveWorkspaceFormat(message) {
    const pending = workspaceFormat;
    if (!pending || message.id !== pending.id) return;
    workspaceFormat = null;
    if (
      typeof message.ok !== "boolean" ||
      (message.error !== undefined && typeof message.error !== "string") ||
      (message.ok && !(message.bytes instanceof Uint8Array))
    ) {
      pending.reject(new Error("invalid workspace formatter response"));
    } else if (message.ok) {
      pending.resolve(message.bytes);
    } else {
      pending.reject(new Error(message.error || "workspace formatting failed"));
    }
  }

  function queueWorkspaceResponse(bytes) {
    return new Promise((resolve, reject) => {
      if (
        disposed ||
        workspaceResponses.length >= 32 ||
        workspaceResponseBytes + bytes.length > 4 * 1024 * 1024
      ) {
        reject(new Error("PolkaVM workspace response queue is unavailable"));
        return;
      }
      workspaceResponses.push({ bytes, resolve, reject });
      workspaceResponseBytes += bytes.length;
      try {
        flushWorkspaceResponses();
        wake();
      } catch (error) {
        workspaceResponses.pop();
        workspaceResponseBytes -= bytes.length;
        reject(error);
      }
    });
  }

  function flushWorkspaceResponses() {
    if (!running || disposed) return;
    while (workspaceResponses.length) {
      const response = workspaceResponses[0];
      if (!sendHostFrameResponse(response.bytes)) return;
      workspaceResponses.shift();
      workspaceResponseBytes -= response.bytes.length;
      response.resolve();
    }
  }

  function failWorkspace(error) {
    if (disposed) return;
    stopRuntime();
    postMessage({ type: "error", message: error.message });
    postMessage({ type: "terminated" });
  }

  function drainMediatedInputCommands() {
    while (true) {
      const operation =
        pvm.polkavm_browser_take_mediated_input_command?.() ?? 0;
      if (operation === 0) {
        return;
      }
      const handle = pvm.polkavm_browser_mediated_input_handle();
      if (operation === 2) {
        postRuntimeOutput({ type: "mediated-input-cancel", handle });
        continue;
      }
      if (operation !== 1) {
        throw new Error(
          "interpreter emitted an invalid mediated-input command",
        );
      }
      const kind = decoder.decode(
        new Uint8Array(
          pvm.memory.buffer,
          pvm.polkavm_browser_mediated_input_kind_pointer(),
          pvm.polkavm_browser_mediated_input_kind_length(),
        ),
      );
      const mediaType = decoder.decode(
        new Uint8Array(
          pvm.memory.buffer,
          pvm.polkavm_browser_mediated_input_media_type_pointer(),
          pvm.polkavm_browser_mediated_input_media_type_length(),
        ),
      );
      postRuntimeOutput({
        type: "mediated-input-request",
        handle,
        kind,
        mediaType,
        maxBytes: pvm.polkavm_browser_mediated_input_max_bytes(),
      });
    }
  }

  function drainAudio() {
    while (pvm.polkavm_browser_take_audio()) {
      if (paused) {
        continue;
      }
      const sampleRate = pvm.polkavm_browser_audio_sample_rate();
      const channels = pvm.polkavm_browser_audio_channels();
      const length = pvm.polkavm_browser_audio_length() * 2;
      const samples = new Uint8Array(
        pvm.memory.buffer,
        pvm.polkavm_browser_audio_pointer(),
        length,
      ).slice();
      postMessage({ type: "audio", sampleRate, channels, samples }, [
        samples.buffer,
      ]);
    }
  }

  function drainSave() {
    while (pvm.polkavm_browser_take_save()) {
      const length = pvm.polkavm_browser_save_length();
      const bytes = new Uint8Array(
        pvm.memory.buffer,
        pvm.polkavm_browser_save_pointer(),
        length,
      ).slice();
      postMessage({ type: "save", bytes }, [bytes.buffer]);
    }
  }

  function drainLogs() {
    while (pvm.polkavm_browser_take_log()) {
      const pointer = pvm.polkavm_browser_log_pointer();
      const length = pvm.polkavm_browser_log_length();
      const message = decoder.decode(
        new Uint8Array(pvm.memory.buffer, pointer, length),
      );
      postMessage({ type: "log", message });
    }
  }

  function drainPointerCapture() {
    let request = null;
    if (translated) {
      request = translated.takePointerCaptureRequest();
    } else {
      const code = pvm.polkavm_browser_take_pointer_capture_request();
      if (code === 1) {
        request = true;
      } else if (code === 2) {
        request = false;
      }
    }
    if (request === null) {
      return;
    }
    postMessage({ type: "pointer-capture", capture: request });
  }

  function scheduleTick(delayMs) {
    if (!running || paused) {
      return;
    }
    clearTimeout(timer);
    timer = undefined;
    if (delayMs <= 0) {
      if (!tickPending) {
        tickPending = true;
        tickChannel.port2.postMessage(null);
      }
      return;
    }
    if (tickPending) {
      return;
    }
    timer = setTimeout(() => {
      timer = undefined;
      if (!running || paused || tickPending) {
        return;
      }
      tickPending = true;
      tickChannel.port2.postMessage(null);
    }, delayMs);
  }

  function wake() {
    if (demandDriven) {
      scheduleTick(0);
    }
  }

  function pauseInput() {
    if (translated) {
      translated.pauseInput();
    } else {
      check(
        pvm.polkavm_browser_pause_input(),
        "discard paused PolkaVM browser input",
      );
    }
    // Releases survive the pause boundary; presses and movement never do.
    // A conforming Host has already sent these, but also release controls if
    // focus disappeared before the Host received their physical key-up.
    for (const bytes of heldInputs.values()) {
      bytes[0] = bytes[0] === 18 ? 21 : bytes[0] + 1;
      sendInput(bytes);
    }
    heldInputs.clear();
    pendingMotionSample = null;
  }

  function setPaused(next) {
    if (typeof next !== "boolean") {
      throw new Error("invalid PolkaVM browser pause state");
    }
    const changed = paused !== next;
    if (changed) {
      const now = performance.now();
      if (next) {
        clearTimeout(timer);
        timer = undefined;
        pendingMotionSample = null;
        if (running) {
          pauseInput();
          pausedAt = now;
        }
      } else if (running) {
        startedAt += now - pausedAt;
      }
      paused = next;
    }
    postMessage({ type: "pause-state", paused });
    if (changed && !paused) {
      scheduleTick(0);
    }
  }

  function requestedUpdateDelay() {
    if (!demandDriven) {
      return LEGACY_FRAME_INTERVAL_MS;
    }
    const delay = translated
      ? translated.updateAfterMilliseconds()
      : pvm.polkavm_browser_update_after_ms();
    return delay === UPDATE_AFTER_IDLE ? null : delay;
  }

  function tick() {
    if (!running || paused) {
      return;
    }
    const firstUpdate = updateCount === 0;
    if (firstUpdate) {
      postMessage({ type: "startup", stage: "first-update-started" });
    }
    const before = performance.now();
    try {
      flushWorkspaceResponses();
      if (translated) {
        translated.update(before - startedAt);
      } else {
        check(
          pvm.polkavm_browser_update(before - startedAt),
          "update PolkaVM browser guest",
        );
        drainFrame();
        drainTri2d();
        drainUiSemantics();
        drainUiOutput();
        drainGpuBatches();
        drainHostFrameRequests();
        drainMediatedInputCommands();
        drainAudio();
        drainSave();
        drainLogs();
      }
      drainPointerCapture();
      flushWorkspaceResponses();
    } catch (error) {
      stopRuntime();
      postMessage({ type: "error", message: error.message });
      postMessage({ type: "terminated" });
      return;
    }
    const elapsed = performance.now() - before;
    if (firstUpdate) {
      postMessage({ type: "startup", stage: "first-update-completed" });
    }
    updateCount++;
    updateSamples.push(elapsed);
    if (updateSamples.length > 600) {
      updateSamples.shift();
    }
    if (updateCount % 120 === 0) {
      const sorted = [...updateSamples].sort((a, b) => a - b);
      const percentile = (value) =>
        sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * value))];
      postMessage({
        type: "metrics",
        updates: updateCount,
        updateP50Ms: percentile(0.5),
        updateP95Ms: percentile(0.95),
        updateMaxMs: sorted[sorted.length - 1],
      });
    }
    const requestedDelay = workspaceResponses.length
      ? 0
      : requestedUpdateDelay();
    if (requestedDelay !== null) {
      scheduleTick(
        demandDriven ? requestedDelay : Math.max(0, requestedDelay - elapsed),
      );
    }
  }

  function asBytes(value, label) {
    if (value instanceof ArrayBuffer) {
      return new Uint8Array(value);
    }
    if (ArrayBuffer.isView(value)) {
      return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    }
    throw new Error(`${label} must be binary data`);
  }

  function validateAssetPath(path) {
    const encoded = encoder.encode(path);
    if (
      !path ||
      encoded.byteLength > MAX_ASSET_NAME_BYTES ||
      path.startsWith("/") ||
      path.includes("\\") ||
      path
        .split("/")
        .some(
          (component) => !component || component === "." || component === "..",
        )
    ) {
      throw new Error(`invalid PolkaVM browser asset path ${path}`);
    }
  }

  function isWebGpuProfile(profile) {
    return profile === "webgpu-raster" || profile === "webgpu";
  }

  function validateStartMessage(message) {
    if (
      !(message.runtime instanceof WebAssembly.Module) &&
      asBytes(message.runtime, "PolkaVM browser runtime").byteLength === 0
    ) {
      throw new Error("PolkaVM browser runtime is empty");
    }
    const program = asBytes(message.program, "PolkaVM browser program");
    if (!program.byteLength || program.byteLength > MAX_PROGRAM_BYTES) {
      throw new Error(
        `PolkaVM browser program must contain 1..=${MAX_PROGRAM_BYTES} bytes`,
      );
    }
    if (
      !["framebuffer", "tri2d", "webgpu-raster", "webgpu"].includes(
        message.graphicsProfile,
      )
    ) {
      throw new Error(
        `invalid PolkaVM browser graphics profile ${message.graphicsProfile}`,
      );
    }
    if (
      !Array.isArray(message.assets) ||
      message.assets.length > MAX_ASSET_FILES
    ) {
      throw new Error(
        `PolkaVM browser launch exceeds ${MAX_ASSET_FILES} assets`,
      );
    }
    const paths = new Set();
    let assetBytes = 0;
    for (const asset of message.assets) {
      if (!asset || typeof asset.path !== "string") {
        throw new Error("PolkaVM browser asset is missing its path");
      }
      validateAssetPath(asset.path);
      if (paths.has(asset.path)) {
        throw new Error(
          `PolkaVM browser asset path is duplicated: ${asset.path}`,
        );
      }
      paths.add(asset.path);
      const length = asBytes(
        asset.bytes,
        `PolkaVM browser asset ${asset.path}`,
      ).byteLength;
      if (length > MAX_ASSET_FILE_BYTES) {
        throw new Error(
          `PolkaVM browser asset ${asset.path} exceeds ${MAX_ASSET_FILE_BYTES} bytes`,
        );
      }
      assetBytes += length;
      if (!Number.isSafeInteger(assetBytes) || assetBytes > MAX_ASSET_BYTES) {
        throw new Error(
          `PolkaVM browser assets exceed ${MAX_ASSET_BYTES} bytes`,
        );
      }
    }
    if (
      isWebGpuProfile(message.graphicsProfile) &&
      !(message.gpuCapabilities instanceof ArrayBuffer)
    ) {
      throw new Error(
        "WebGPU capabilities are required before PolkaVM initialization",
      );
    }
    if (
      message.motionAvailability !== undefined &&
      (!Number.isInteger(message.motionAvailability) ||
        message.motionAvailability < 0 ||
        message.motionAvailability > 2)
    ) {
      throw new Error("invalid PolkaVM browser motion availability");
    }
    const mediatedInputKinds = message.mediatedInputKinds ?? [];
    if (
      !Array.isArray(mediatedInputKinds) ||
      mediatedInputKinds.length > MAX_MEDIATED_INPUT_REGISTRATIONS ||
      mediatedInputKinds.some(
        (kind) =>
          typeof kind !== "string" ||
          encoder.encode(kind).byteLength > MAX_MEDIATED_INPUT_KIND_BYTES ||
          !/^[a-z0-9][a-z0-9+._-]*[a-z0-9]$|^[a-z0-9]$/.test(kind),
      ) ||
      new Set(mediatedInputKinds).size !== mediatedInputKinds.length
    ) {
      throw new Error("invalid PolkaVM browser mediated-input kinds");
    }
    return program;
  }

  async function start(message) {
    if (disposed) {
      throw new Error("PolkaVM browser worker is stopped");
    }
    if (starting || pvm || running) {
      throw new Error("PolkaVM browser worker is already started");
    }
    const program = validateStartMessage(message);
    if (message.workspace != null && !globalThis.PolkaVmWorkspace) {
      throw new Error("PolkaVM workspace service is not installed");
    }
    if (globalThis.PolkaVmWorkspace) {
      workspace = new globalThis.PolkaVmWorkspace.WorkspaceService(
        message.workspace,
        {
          commit: commitWorkspace,
          format:
            message.workspaceFormat === true ? formatWorkspace : undefined,
          respond: queueWorkspaceResponse,
          fatal: failWorkspace,
        },
      );
    }
    starting = true;
    motionAvailability = message.motionAvailability ?? 0;
    pointerCaptureSupported = message.pointerCaptureSupported === true;
    pendingGpuCapabilities =
      message.gpuCapabilities instanceof ArrayBuffer
        ? new Uint8Array(message.gpuCapabilities).slice()
        : null;
    const bootStarted = performance.now();
    let translationMs = 0;
    let compilationMs = 0;
    let translatedWasmBytes = 0;
    let cacheHit = false;
    let compilerStage = "compiler-staging-program";
    let compilerFallbackReason;
    let compilerFallbackStage;
    postMessage({ type: "startup", stage: "runtime-instantiating" });
    const runtimeImports = {
      polkavm_browser: {
        clock_wall_ms: () => Date.now(),
        random_fill: (pointer, length) => {
          if (pvm == null) {
            return CORE_STATUS_DENIED;
          }
          try {
            const browserCrypto = globalThis.crypto;
            if (typeof browserCrypto?.getRandomValues !== "function") {
              return CORE_STATUS_DENIED;
            }
            const bytes = new Uint8Array(length >>> 0);
            browserCrypto.getRandomValues(bytes);
            new Uint8Array(pvm.memory.buffer, pointer >>> 0, length >>> 0).set(
              bytes,
            );
            return 0;
          } catch {
            return CORE_STATUS_DENIED;
          }
        },
      },
    };
    pvm = (await WebAssembly.instantiate(message.runtime, runtimeImports))
      .instance.exports;
    if (disposed) {
      pvm.polkavm_browser_reset?.();
      return;
    }
    if (pvm.polkavm_browser_abi_version() !== 2) {
      throw new Error("PolkaVM browser runtime has an incompatible ABI");
    }
    postMessage({ type: "startup", stage: "runtime-instantiated" });
    const pendingOutputs = [];
    try {
      if (message.forceInterpreter === true) {
        throw FORCE_INTERPRETER;
      }
      stage(program);
      let compiledProgram = message.compiledProgram;
      let bytes =
        message.compiledBytes instanceof ArrayBuffer
          ? new Uint8Array(message.compiledBytes)
          : null;
      const hasCompiledProgram =
        globalThis.TranslatedPolkaVmRuntime.isCompiledProgram(compiledProgram);
      cacheHit = hasCompiledProgram || bytes !== null;
      if (!hasCompiledProgram) {
        let generatedTranslation = bytes === null;
        if (bytes === null) {
          compilerStage = "compiler-translating";
          const translationStarted = performance.now();
          check(
            pvm.polkavm_browser_translate_staged(),
            "translate PolkaVM browser guest",
          );
          translationMs = performance.now() - translationStarted;
          const pointer = pvm.polkavm_browser_translation_pointer();
          const length = pvm.polkavm_browser_translation_length();
          bytes = new Uint8Array(pvm.memory.buffer, pointer, length).slice();
        }
        translatedWasmBytes = bytes.byteLength;
        // Translation can grow its linear memory far beyond the guest heap.
        // The compiled backend does not use that instance; release it before
        // the browser allocates native code for the root and its code parts.
        pvm = null;
        compilerStage = "compiler-compiling";
        const compilationStarted = performance.now();
        try {
          compiledProgram =
            await globalThis.TranslatedPolkaVmRuntime.compile(bytes);
          compilationMs = performance.now() - compilationStarted;
          if (disposed) {
            return;
          }
        } catch (error) {
          compilationMs = performance.now() - compilationStarted;
          if (disposed) {
            return;
          }
          // Invalid Wasm cannot be repaired by changing compilation-unit sizes.
          if (error instanceof WebAssembly.CompileError) {
            throw error;
          }
          console.warn(
            `PolkaVM single-module compilation failed; compiling bounded code parts: ${error instanceof Error ? error.message : String(error)}`,
          );
          compilerStage = "compiler-translating-parts";
          pvm = (await WebAssembly.instantiate(message.runtime, runtimeImports))
            .instance.exports;
          if (disposed) {
            pvm.polkavm_browser_reset?.();
            return;
          }
          stage(program);
          const translationStarted = performance.now();
          check(
            pvm.polkavm_browser_translate_partitioned_staged(),
            "translate bounded PolkaVM browser code parts",
          );
          translationMs += performance.now() - translationStarted;
          bytes = new Uint8Array(
            pvm.memory.buffer,
            pvm.polkavm_browser_translation_pointer(),
            pvm.polkavm_browser_translation_length(),
          ).slice();
          generatedTranslation = true;
          translatedWasmBytes = bytes.byteLength;
          pvm = null;
          compilerStage = "compiler-compiling-parts";
          const partsStarted = performance.now();
          try {
            compiledProgram =
              await globalThis.TranslatedPolkaVmRuntime.compile(bytes);
          } finally {
            compilationMs += performance.now() - partsStarted;
          }
        }
        if (disposed) {
          return;
        }
        if (generatedTranslation) {
          const persistent = bytes.slice();
          postMessage(
            {
              type: "translated",
              cacheKey: message.cacheKey,
              bytes: persistent,
            },
            [persistent.buffer],
          );
        }
        try {
          postMessage({
            type: "compiled",
            cacheKey: message.cacheKey,
            program: compiledProgram,
          });
        } catch {}
      }
      pvm = null;
      compilerStage = "compiler-instantiating";
      translated = new globalThis.TranslatedPolkaVmRuntime(
        compiledProgram,
        message.assets,
        (output, transfers = []) => {
          if (running) {
            postRuntimeOutput(output, transfers);
          } else {
            pendingOutputs.push({ output, transfers });
          }
        },
        MAX_TRANSLATED_LOOPS_PER_UPDATE,
        message.audioEnabled,
        message.graphicsProfile,
        pendingGpuCapabilities,
        motionAvailability,
        message.mediatedInputKinds ?? [],
      );
      compilerStage = "compiler-initializing";
      if (pendingMotionSample !== null) {
        translated.sendMotionSample(pendingMotionSample);
      }
      translated.setPointerCaptureSupported(pointerCaptureSupported);
      translated.initialize(MAX_GAS_PER_UPDATE);
      pendingGpuCapabilities = null;
      pendingMotionSample = null;
      backend = "compiler";
    } catch (error) {
      translated = null;
      if (disposed) {
        return;
      }
      pendingOutputs.length = 0;
      if (error !== FORCE_INTERPRETER) {
        compilerFallbackReason =
          error instanceof Error ? error.message : String(error);
        compilerFallbackStage = compilerStage;
        console.warn(
          `PolkaVM ${compilerFallbackStage} failed; using interpreter: ${compilerFallbackReason}`,
        );
      }
      if (pvm === null) {
        pvm = (await WebAssembly.instantiate(message.runtime, runtimeImports))
          .instance.exports;
        if (disposed) {
          pvm.polkavm_browser_reset?.();
          return;
        }
      }
      let presentation = 0;
      if (message.graphicsProfile === "tri2d") {
        presentation = 1;
      } else if (message.graphicsProfile === "webgpu-raster") {
        presentation = 2;
      } else if (message.graphicsProfile === "webgpu") {
        presentation = 3;
      }
      const begin = pvm.polkavm_browser_launch_begin_v2;
      if (typeof begin !== "function") {
        throw new Error(
          "PolkaVM interpreter does not support graphics profiles",
        );
      }
      postMessage({ type: "startup", stage: "interpreter-staging-program" });
      stage(program);
      postMessage({ type: "startup", stage: "interpreter-program-staged" });
      postMessage({ type: "startup", stage: "interpreter-launch-begin" });
      check(
        begin(MAX_GAS_PER_UPDATE, message.audioEnabled ? 1 : 0, presentation),
        "begin PolkaVM browser launch",
      );
      postMessage({ type: "startup", stage: "interpreter-launch-begun" });
      postMessage({ type: "startup", stage: "interpreter-mounting-assets" });
      for (const asset of message.assets) {
        addAsset(asset);
      }
      postMessage({ type: "startup", stage: "interpreter-assets-mounted" });
      postMessage({ type: "startup", stage: "interpreter-launch-starting" });
      check(pvm.polkavm_browser_launch_start(), "start PolkaVM browser launch");
      postMessage({ type: "startup", stage: "interpreter-launch-started" });
      const setRandomBytes = pvm.polkavm_browser_set_random_bytes;
      if (typeof setRandomBytes !== "function") {
        throw new Error("PolkaVM interpreter does not support secure random");
      }
      const randomBytes = executionRandomBytes();
      if (randomBytes.byteLength > 0) {
        stage(randomBytes);
        check(setRandomBytes(), "set PolkaVM browser random bytes");
      }
      check(
        pvm.polkavm_browser_set_motion_availability(motionAvailability),
        "set PolkaVM browser motion availability",
      );
      check(
        pvm.polkavm_browser_set_pointer_capture_supported(
          pointerCaptureSupported ? 1 : 0,
        ),
        "set PolkaVM browser pointer capture support",
      );
      if ((message.mediatedInputKinds?.length ?? 0) > 0) {
        stage(encoder.encode(message.mediatedInputKinds.join("\0")));
        check(
          pvm.polkavm_browser_set_mediated_input_kinds(),
          "set PolkaVM browser mediated-input kinds",
        );
      }
      if (pendingMotionSample !== null) {
        stage(pendingMotionSample);
        check(
          pvm.polkavm_browser_send_motion_sample(),
          "send PolkaVM browser motion sample",
        );
        pendingMotionSample = null;
      }
      if (isWebGpuProfile(message.graphicsProfile)) {
        if (pendingGpuCapabilities === null) {
          throw new Error(
            "WebGPU capabilities are required before PolkaVM initialization",
          );
        }
        stage(pendingGpuCapabilities);
        check(
          pvm.polkavm_browser_set_gpu_capabilities(),
          "set PolkaVM browser GPU capabilities",
        );
        pendingGpuCapabilities = null;
      }
      postMessage({ type: "startup", stage: "interpreter-initializing" });
      try {
        check(pvm.polkavm_browser_init(), "initialize PolkaVM browser guest");
      } catch (initError) {
        drainLogs();
        throw initError;
      }
      drainMediatedInputCommands();
      postMessage({ type: "startup", stage: "interpreter-initialized" });
      drainTri2d();
      drainUiOutput();
      drainGpuBatches();
      drainHostFrameRequests();
      drainLogs();
    }
    if (disposed) {
      translated?.stop();
      pvm?.polkavm_browser_reset?.();
      return;
    }
    const usesMotion = translated
      ? translated.usesMotion()
      : pvm.polkavm_browser_uses_motion() === 1;
    const usesPointerCapture = translated
      ? translated.usesPointerCapture()
      : pvm.polkavm_browser_uses_pointer_capture() === 1;
    demandDriven = translated
      ? translated.usesUpdateScheduling()
      : typeof pvm.polkavm_browser_uses_update_scheduling === "function" &&
        pvm.polkavm_browser_uses_update_scheduling() === 1;
    startedAt = performance.now();
    starting = false;
    if (paused) {
      pausedAt = startedAt;
      pauseInput();
    }
    running = true;
    postMessage({
      type: "ready",
      backend,
      compilerFallbackReason,
      compilerFallbackStage,
      usesMotion,
      usesPointerCapture,
      usesUpdateScheduling: demandDriven,
      cacheHit,
      translationMs,
      compilationMs,
      translatedWasmBytes,
      startupMs: performance.now() - bootStarted,
    });
    for (const { output, transfers } of pendingOutputs) {
      postRuntimeOutput(output, transfers);
    }
    flushWorkspaceResponses();
    scheduleTick(0);
  }

  function sendInput(bytes) {
    if (bytes.byteLength !== 8) {
      return;
    }
    if (
      bytes[0] === INPUT_SAFE_AREA_INSETS ||
      bytes[0] === INPUT_KEYBOARD_INSETS
    ) {
      throw new Error(
        "viewport insets must be sent with the view-insets message",
      );
    }
    if (!running) {
      return;
    }
    const type = bytes[0];
    if (
      paused &&
      !(
        type === 2 ||
        type === 4 ||
        type === 7 ||
        type === 12 ||
        type === 20 ||
        type === 21 ||
        ((type === 13 || type === 15) && bytes[1] === 0)
      )
    ) {
      return;
    }
    if (type === 1 || type === 3 || type === 18) {
      const key = type * 256 + bytes[1];
      if (!heldInputs.has(key)) {
        heldInputs.set(key, bytes.slice());
      }
    } else if (type === 2 || type === 4) {
      heldInputs.delete((type - 1) * 256 + bytes[1]);
    } else if (type === 20 || type === 21) {
      heldInputs.delete(18 * 256 + bytes[1]);
    }
    if (translated) {
      translated.sendInput(bytes);
      return;
    }
    if (bytes[0] <= 7) {
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      check(
        pvm.polkavm_browser_send_input(
          bytes[0],
          bytes[1],
          view.getUint16(2, true),
          view.getUint16(4, true),
        ),
        "send PolkaVM browser input",
      );
      return;
    }
    stage(bytes);
    check(
      pvm.polkavm_browser_send_input_record(),
      "send PolkaVM browser extended input",
    );
  }

  function sendViewInsets(eventType, left, top, right, bottom) {
    if (
      eventType !== INPUT_SAFE_AREA_INSETS &&
      eventType !== INPUT_KEYBOARD_INSETS
    ) {
      throw new Error("invalid PolkaVM browser inset event type");
    }
    for (const value of [left, top, right, bottom]) {
      if (!Number.isInteger(value) || value < 0 || value > MAX_INSET_PIXELS) {
        throw new Error("invalid PolkaVM browser inset value");
      }
    }
    if (!running) {
      return;
    }
    if (translated) {
      translated.sendViewInsets(eventType, left, top, right, bottom);
      return;
    }
    if (typeof pvm.polkavm_browser_send_view_insets !== "function") {
      throw new Error("PolkaVM browser runtime has an incompatible ABI");
    }
    check(
      pvm.polkavm_browser_send_view_insets(eventType, left, top, right, bottom),
      "send PolkaVM browser view insets",
    );
  }

  function setMotionAvailability(availability) {
    if (
      !Number.isInteger(availability) ||
      availability < 0 ||
      availability > 2
    ) {
      throw new Error("invalid PolkaVM browser motion availability");
    }
    motionAvailability = availability;
    if (availability !== 1) {
      pendingMotionSample = null;
    }
    if (!running) {
      return;
    }
    if (translated) {
      translated.setMotionAvailability(availability);
      return;
    }
    check(
      pvm.polkavm_browser_set_motion_availability(availability),
      "set PolkaVM browser motion availability",
    );
  }

  function setPointerCaptureSupported(supported) {
    pointerCaptureSupported = supported === true;
    if (!running) {
      return;
    }
    if (translated) {
      translated.setPointerCaptureSupported(pointerCaptureSupported);
      return;
    }
    check(
      pvm.polkavm_browser_set_pointer_capture_supported(
        pointerCaptureSupported ? 1 : 0,
      ),
      "set PolkaVM browser pointer capture support",
    );
  }

  function setPointerCaptureActive(active) {
    if (!running) {
      return;
    }
    if (translated) {
      translated.setPointerCaptureActive(active === true);
      return;
    }
    check(
      pvm.polkavm_browser_set_pointer_capture_active(active === true ? 1 : 0),
      "report PolkaVM browser pointer capture state",
    );
  }

  function sendMotionSample(bytes) {
    if (bytes.byteLength !== MOTION_SAMPLE_BYTES) {
      throw new Error("invalid PolkaVM browser motion sample");
    }
    if (paused) {
      return;
    }
    if (!running) {
      pendingMotionSample = bytes.slice();
      motionAvailability = 1;
      return;
    }
    if (translated) {
      translated.sendMotionSample(bytes);
      return;
    }
    stage(bytes);
    check(
      pvm.polkavm_browser_send_motion_sample(),
      "send PolkaVM browser motion sample",
    );
  }

  function sendGpuCapabilities(bytes) {
    if (bytes.byteLength < 56 || bytes.byteLength > 4096) {
      throw new Error("invalid PolkaVM browser GPU capabilities");
    }
    if (!running) {
      pendingGpuCapabilities = bytes.slice();
      return;
    }
    if (translated) {
      translated.setGpuCapabilities(bytes);
      return;
    }
    stage(bytes);
    check(
      pvm.polkavm_browser_set_gpu_capabilities(),
      "update PolkaVM browser GPU capabilities",
    );
  }

  function sendGpuEvent(bytes) {
    if (!running || !bytes.byteLength) {
      return;
    }
    if (translated) {
      translated.sendGpuEvent(bytes);
      return;
    }
    stage(bytes);
    check(
      pvm.polkavm_browser_send_gpu_event(),
      "send PolkaVM browser GPU event",
    );
  }

  function sendHostFrameResponse(bytes) {
    if (!running || !bytes.byteLength) {
      return true;
    }
    if (translated) {
      return translated.sendHostFrameResponse(bytes);
    }
    stage(bytes);
    const result = pvm.polkavm_browser_send_host_frame_response();
    if (result === 2) {
      return false;
    }
    check(result, "send PolkaVM browser host-frame response");
    return true;
  }

  function sendMediatedInputResult(handle, status, bytes) {
    if (
      !running ||
      (!translated && !pvm) ||
      !Number.isInteger(handle) ||
      handle <= 0 ||
      !Number.isInteger(status) ||
      status < 3 ||
      status > 6
    ) {
      throw new Error("invalid PolkaVM browser mediated-input result");
    }
    if (translated) {
      translated.sendMediatedInputResult(handle, status, bytes);
      activeMediatedInputHandles.delete(handle);
      return;
    }
    stage(status === 3 ? bytes : new Uint8Array([0]));
    check(
      pvm.polkavm_browser_send_mediated_input_result(handle, status),
      "send PolkaVM browser mediated-input result",
    );
    activeMediatedInputHandles.delete(handle);
  }
  endpoint.onmessage = (event) => {
    const message = event.data;
    if (message?.type === "start") {
      void start(message).catch((error) => {
        if (disposed) {
          return;
        }
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      });
    } else if (message?.type === "pause") {
      try {
        setPaused(message.paused);
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "input") {
      try {
        sendInput(new Uint8Array(message.bytes));
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "view-insets") {
      try {
        sendViewInsets(
          message.eventType,
          message.left,
          message.top,
          message.right,
          message.bottom,
        );
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "motion-status") {
      try {
        setMotionAvailability(message.availability);
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "pointer-capture-support") {
      try {
        if (typeof message.supported !== "boolean") {
          throw new Error("invalid PolkaVM browser pointer capture support");
        }
        setPointerCaptureSupported(message.supported);
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "pointer-capture-state") {
      try {
        if (typeof message.active !== "boolean") {
          throw new Error("invalid PolkaVM browser pointer capture state");
        }
        setPointerCaptureActive(message.active);
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "motion") {
      try {
        sendMotionSample(new Uint8Array(message.bytes));
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "gpu-capabilities") {
      try {
        sendGpuCapabilities(new Uint8Array(message.bytes));
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "gpu-event") {
      try {
        sendGpuEvent(new Uint8Array(message.bytes));
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "host-frame-response") {
      try {
        const seq = message.seq;
        if (seq !== undefined && (!Number.isSafeInteger(seq) || seq < 0)) {
          throw new Error(
            "invalid PolkaVM browser host frame response sequence",
          );
        }
        if (sendHostFrameResponse(new Uint8Array(message.bytes))) {
          wake();
          if (seq !== undefined) {
            postMessage({ type: "host-frame-response-accepted", seq });
          }
        } else if (seq !== undefined) {
          postMessage({
            type: "host-frame-response-rejected",
            reason: "queue-full",
            seq,
          });
        } else {
          postMessage({
            type: "host-frame-response-rejected",
            reason: "queue-full",
          });
        }
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "workspace-commit-result") {
      receiveWorkspaceCommit(message);
    } else if (message?.type === "workspace-format-result") {
      receiveWorkspaceFormat(message);
    } else if (message?.type === "mediated-input-result") {
      try {
        sendMediatedInputResult(
          message.handle,
          message.status,
          new Uint8Array(message.bytes),
        );
        wake();
      } catch (error) {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      }
    } else if (message?.type === "stop") {
      stopRuntime();
      postMessage({ type: "terminated" });
    }
  };
};

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

globalThis.createPolkaVmRuntime(globalThis);
