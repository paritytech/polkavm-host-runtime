/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

"use strict";

/**
 * Creates the bounded PolkaVM endpoint in a Worker or on a host thread.
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
  const MAX_PROGRAM_BYTES = 64 * 1024 * 1024;
  const MAX_ASSET_FILES = 2048;
  const MAX_ASSET_NAME_BYTES = 1024;
  const MAX_ASSET_FILE_BYTES = 128 * 1024 * 1024;
  const MAX_ASSET_BYTES = 256 * 1024 * 1024;
  const MOTION_SAMPLE_BYTES = 48;
  const MAX_MEDIATED_INPUT_KIND_BYTES = 32;
  const MAX_MEDIATED_INPUT_REGISTRATIONS = 8;
  const MAX_RELAUNCH_FILE_BYTES = 128 * 1024 * 1024;
  const FILE_INPUT_OUTCOMES = ["ready", "error", "", "rejected", "refused", "relaunch"];
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

  let pvm;
  let translated;
  let backend = "interpreter";
  let running = false;
  let disposed = false;
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
  // The newest registration snapshot a Host has not received yet. It is
  // flushed on teardown so a Host keeps the registrations of an execution
  // that failed before becoming ready.
  let unpostedFileRegistrations = null;
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
    if (unpostedFileRegistrations !== null) {
      postMessage(unpostedFileRegistrations);
      unpostedFileRegistrations = null;
    }
    for (const handle of activeMediatedInputHandles) {
      postMessage({ type: "mediated-input-cancel", handle });
    }
    activeMediatedInputHandles.clear();
    running = false;
    clearTimeout(timer);
    translated?.stop();
    pvm?.polkavm_browser_reset?.();
    tickChannel.port1.close();
    tickChannel.port2.close();
    endpoint.onmessage = null;
  }

  function postRuntimeOutput(output, transfers = []) {
    if (
      output?.type === "mediated-input-request" ||
      output?.type === "file-input-request"
    ) {
      activeMediatedInputHandles.add(output.handle);
    } else if (output?.type === "mediated-input-cancel") {
      activeMediatedInputHandles.delete(output.handle);
    } else if (output?.type === "file-registrations") {
      unpostedFileRegistrations = null;
    }
    postMessage(output, transfers);
  }

  function wasmJson(pointer, length) {
    return JSON.parse(
      decoder.decode(new Uint8Array(pvm.memory.buffer, pointer, length)),
    );
  }

  function drainFileRegistrations() {
    if (!pvm.polkavm_browser_take_file_registrations?.()) {
      return;
    }
    postRuntimeOutput({
      type: "file-registrations",
      registrations: wasmJson(
        pvm.polkavm_browser_file_registrations_pointer(),
        pvm.polkavm_browser_file_registrations_length(),
      ),
    });
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
      postMessage({ type: "host-frame-request", bytes }, [bytes.buffer]);
    }
  }

  function drainMediatedInputCommands() {
    while (true) {
      const operation = pvm.polkavm_browser_take_mediated_input_command?.() ?? 0;
      if (operation === 0) {
        return;
      }
      const handle = pvm.polkavm_browser_mediated_input_handle();
      if (operation === 2) {
        postRuntimeOutput({ type: "mediated-input-cancel", handle });
        continue;
      }
      if (operation === 3) {
        postRuntimeOutput({
          type: "file-input-request",
          handle,
          descriptor: wasmJson(
            pvm.polkavm_browser_mediated_input_descriptor_pointer(),
            pvm.polkavm_browser_mediated_input_descriptor_length(),
          ),
        });
        continue;
      }
      if (operation !== 1) {
        throw new Error("interpreter emitted an invalid mediated-input command");
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
    if (!running) {
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
      if (!running || tickPending) {
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
    if (!running) {
      return;
    }
    const firstUpdate = updateCount === 0;
    if (firstUpdate) {
      postMessage({ type: "startup", stage: "first-update-started" });
    }
    const before = performance.now();
    try {
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
        drainFileRegistrations();
        drainMediatedInputCommands();
        drainAudio();
        drainSave();
        drainLogs();
      }
      drainPointerCapture();
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
    const requestedDelay = requestedUpdateDelay();
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
    const fileInput = message.fileInput;
    if (
      fileInput !== undefined &&
      (typeof fileInput?.inline !== "boolean" ||
        typeof fileInput.relaunch !== "boolean" ||
        typeof fileInput.entrypoint !== "string" ||
        ((fileInput.inline || fileInput.relaunch) && !fileInput.entrypoint))
    ) {
      throw new Error("invalid PolkaVM browser file-input support");
    }
    const relaunch = message.fileRelaunch;
    if (relaunch !== undefined) {
      if (
        ["id", "mountPath", "name", "mimeType"].some(
          (field) => typeof relaunch?.[field] !== "string",
        )
      ) {
        throw new Error("invalid PolkaVM browser relaunch file");
      }
      validateAssetPath(relaunch.mountPath);
      const length = asBytes(
        relaunch.bytes,
        "PolkaVM browser relaunch file",
      ).byteLength;
      const replaced = message.assets.find(
        (asset) => asset.path === relaunch.mountPath,
      );
      assetBytes +=
        length - (replaced ? asBytes(replaced.bytes, "asset").byteLength : 0);
      if (
        !length ||
        length > MAX_RELAUNCH_FILE_BYTES ||
        paths.size + (replaced ? 0 : 1) > MAX_ASSET_FILES ||
        assetBytes > MAX_ASSET_BYTES
      ) {
        throw new Error("PolkaVM browser relaunch file exceeds the asset bounds");
      }
    }
    return program;
  }

  function relaunchFile(message) {
    const relaunch = message.fileRelaunch;
    return relaunch === undefined
      ? null
      : {
          id: relaunch.id,
          mountPath: relaunch.mountPath,
          name: relaunch.name,
          mimeType: relaunch.mimeType,
          bytes: asBytes(relaunch.bytes, "PolkaVM browser relaunch file"),
        };
  }

  async function start(message) {
    if (disposed) {
      throw new Error("PolkaVM browser worker is stopped");
    }
    if (pvm || running) {
      throw new Error("PolkaVM browser worker is already started");
    }
    const program = validateStartMessage(message);
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
            new Uint8Array(
              pvm.memory.buffer,
              pointer >>> 0,
              length >>> 0,
            ).set(bytes);
            return 0;
          } catch {
            return CORE_STATUS_DENIED;
          }
        },
      },
    };
    pvm = (
      await WebAssembly.instantiate(message.runtime, runtimeImports)
    ).instance.exports;
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
        } catch (error) {
          compilationMs = performance.now() - compilationStarted;
          // Invalid Wasm cannot be repaired by changing compilation-unit sizes.
          if (error instanceof WebAssembly.CompileError) {
            throw error;
          }
          console.warn(
            `PolkaVM single-module compilation failed; compiling bounded code parts: ${error instanceof Error ? error.message : String(error)}`,
          );
          compilerStage = "compiler-translating-parts";
          pvm = (
            await WebAssembly.instantiate(message.runtime, runtimeImports)
          ).instance.exports;
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
        if (generatedTranslation) {
          const persistent = bytes.slice();
          postMessage(
            { type: "translated", cacheKey: message.cacheKey, bytes: persistent },
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
            if (output?.type === "file-registrations") {
              unpostedFileRegistrations = output;
            }
          }
        },
        MAX_TRANSLATED_LOOPS_PER_UPDATE,
        message.audioEnabled,
        message.graphicsProfile,
        pendingGpuCapabilities,
        motionAvailability,
        message.mediatedInputKinds ?? [],
        message.fileInput ?? null,
      );
      const relaunch = relaunchFile(message);
      if (relaunch !== null) {
        translated.setFileRelaunch(relaunch);
      }
      compilerStage = "compiler-initializing";
      if (pendingMotionSample !== null) {
        translated.sendMotionSample(pendingMotionSample);
      }
      translated.setPointerCaptureSupported(pointerCaptureSupported);
      translated.initialize();
      pendingGpuCapabilities = null;
      pendingMotionSample = null;
      backend = "compiler";
    } catch (error) {
      translated = null;
      pendingOutputs.length = 0;
      unpostedFileRegistrations = null;
      if (error !== FORCE_INTERPRETER) {
        compilerFallbackReason =
          error instanceof Error ? error.message : String(error);
        compilerFallbackStage = compilerStage;
        console.warn(
          `PolkaVM ${compilerFallbackStage} failed; using interpreter: ${compilerFallbackReason}`,
        );
      }
      if (pvm === null) {
        pvm = (
          await WebAssembly.instantiate(message.runtime, runtimeImports)
        ).instance.exports;
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
      if (message.fileInput?.inline || message.fileInput?.relaunch) {
        stage(encoder.encode(message.fileInput.entrypoint));
        check(
          pvm.polkavm_browser_set_file_input_support(
            message.fileInput.inline ? 1 : 0,
            message.fileInput.relaunch ? 1 : 0,
          ),
          "set PolkaVM browser file-input support",
        );
      }
      const relaunch = relaunchFile(message);
      if (relaunch !== null) {
        const { bytes, ...metadata } = relaunch;
        stage(encoder.encode(JSON.stringify(metadata)));
        check(
          pvm.polkavm_browser_stage_file_metadata(),
          "stage PolkaVM browser relaunch file",
        );
        stage(bytes);
        check(
          pvm.polkavm_browser_set_file_relaunch(),
          "mount PolkaVM browser relaunch file",
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
        drainFileRegistrations();
        throw initError;
      }
      drainFileRegistrations();
      drainMediatedInputCommands();
      postMessage({ type: "startup", stage: "interpreter-initialized" });
      drainTri2d();
      drainUiOutput();
      drainGpuBatches();
      drainHostFrameRequests();
      drainLogs();
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
  /**
   * Delivers a file the user selected in Host UI and reports the outcome. A
   * relaunch delivery stops this execution.
   */
  function sendFileInput(handle, name, mimeType, bytes) {
    if (
      !running ||
      !Number.isInteger(handle) ||
      handle <= 0 ||
      typeof name !== "string" ||
      typeof mimeType !== "string"
    ) {
      throw new Error("invalid PolkaVM browser file input");
    }
    let delivery;
    if (translated) {
      delivery = translated.deliverFile(handle, name, mimeType, bytes);
    } else {
      stage(encoder.encode(JSON.stringify({ name, mimeType })));
      check(
        pvm.polkavm_browser_stage_file_metadata(),
        "stage PolkaVM browser file input",
      );
      if (bytes.byteLength) {
        stage(bytes);
      }
      const code = pvm.polkavm_browser_send_file_input(handle);
      const outcome = FILE_INPUT_OUTCOMES[code];
      if (!outcome || outcome === "error") {
        throw new Error(`send PolkaVM browser file input: ${errorText()}`);
      }
      delivery = { outcome };
      if (outcome === "relaunch") {
        delivery.relaunch = {
          ...wasmJson(
            pvm.polkavm_browser_file_relaunch_pointer(),
            pvm.polkavm_browser_file_relaunch_length(),
          ),
          bytes,
        };
      }
    }
    if (delivery.outcome !== "refused") {
      activeMediatedInputHandles.delete(handle);
    }
    if (delivery.outcome !== "relaunch") {
      postMessage({ type: "file-input-delivery", handle, outcome: delivery.outcome });
      return false;
    }
    const relaunch = { ...delivery.relaunch, bytes: delivery.relaunch.bytes.slice() };
    postMessage(
      { type: "file-input-delivery", handle, outcome: "relaunch", relaunch },
      [relaunch.bytes.buffer],
    );
    return true;
  }

  endpoint.onmessage = (event) => {
    const message = event.data;
    if (message?.type === "start") {
      void start(message).catch((error) => {
        stopRuntime();
        postMessage({ type: "error", message: error.message });
        postMessage({ type: "terminated" });
      });
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
    } else if (message?.type === "file-input") {
      try {
        if (
          sendFileInput(
            message.handle,
            message.name,
            message.mimeType,
            asBytes(message.bytes, "PolkaVM browser file input"),
          )
        ) {
          stopRuntime();
          postMessage({ type: "terminated" });
        } else {
          wake();
        }
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
