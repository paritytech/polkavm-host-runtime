/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Start one owned, single-use Worker. Binary inputs are cloned, not transferred.
 * Hosts own presentation, permissions, output resources, and cache persistence.
 * @param {import('./session.js').SessionOptions} options
 * @returns {import('./session.js').BrowserSession}
 */
export function startSession(options) {
  let worker;
  let state = "starting";
  let stopTimer;
  let onOutput = options.onOutput;
  let resolveReady;
  let rejectReady;
  let resolveTerminal;
  const ready = new Promise((resolve, reject) => {
    resolveReady = resolve;
    rejectReady = reject;
  });
  // Stopping before a caller awaits ready must not cause an unhandled rejection.
  void ready.catch(() => {});
  const terminal = new Promise((resolve) => { resolveTerminal = resolve; });
  const asError = (error) => error instanceof Error ? error : new Error(String(error));

  function finish(result) {
    if (state === "terminated") return;
    state = "terminated";
    clearTimeout(stopTimer);
    if (worker) {
      worker.removeEventListener("message", receive);
      worker.removeEventListener("error", workerError);
      worker.removeEventListener("messageerror", messageError);
      worker.terminate();
      worker = undefined;
    }
    onOutput = undefined;
    rejectReady(result.error ?? new DOMException("Session ended before ready", "AbortError"));
    resolveTerminal(result);
  }

  function receive(event) {
    if (state === "terminated") return;
    const output = event.data;
    if (output.type === "ready" && state === "starting") {
      state = "running";
      resolveReady(output);
    }
    // A late ready/frame cannot resurrect a session after stop was requested.
    if (state === "stopping" && output.type !== "mediated-input-cancel" &&
        output.type !== "error" && output.type !== "terminated") return;
    const stopping = state === "stopping";
    try {
      onOutput?.(output);
    } catch (error) {
      finish({ reason: "error", error: asError(error) });
      return;
    }
    if (output.type === "error") {
      finish({ reason: "error", error: new Error(output.message) });
    } else if (output.type === "terminated") {
      finish({ reason: stopping ? "stopped" : "terminated" });
    }
  }

  function workerError(event) {
    event.preventDefault();
    finish({ reason: "error", error: new Error(event.message || "Session worker failed") });
  }

  function messageError() {
    finish({ reason: "error", error: new Error("Session worker message could not be decoded") });
  }

  const session = {
    ready,
    terminal,
    get state() { return state; },
    send(input) {
      if (state !== "running") {
        throw new Error(`Cannot send input to a ${state} session; await ready first`);
      }
      if (input?.type === "start" || input?.type === "stop") {
        throw new TypeError("Use the session lifecycle, not a start/stop input");
      }
      try {
        worker.postMessage(input);
      } catch (error) {
        finish({ reason: "error", error: asError(error) });
        throw error;
      }
    },
    stop() {
      if (state === "terminated" || state === "stopping") return terminal;
      state = "stopping";
      rejectReady(new DOMException("Session stopped before ready", "AbortError"));
      // Normally the worker cancels active mediated inputs and acknowledges stop.
      // A busy or broken guest must not retain a Worker indefinitely.
      stopTimer = setTimeout(() => finish({ reason: "stopped" }), 1000);
      try {
        worker.postMessage({ type: "stop" });
      } catch (error) {
        finish({ reason: "error", error: asError(error) });
      }
      return terminal;
    },
  };

  try {
    const { workerUrl, ...startup } = options;
    delete startup.onOutput;
    worker = new Worker(workerUrl ?? new URL("./polkavm-worker.js", import.meta.url), {
      type: "classic",
      name: "polkavm-session",
    });
    worker.addEventListener("message", receive);
    worker.addEventListener("error", workerError);
    worker.addEventListener("messageerror", messageError);
    worker.postMessage({ ...startup, type: "start" });
  } catch (error) {
    finish({ reason: "error", error: asError(error) });
  }
  return session;
}
