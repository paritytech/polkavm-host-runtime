/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

export type Binary = ArrayBuffer | Uint8Array;
export type GraphicsProfile = "framebuffer" | "tri2d" | "webgpu-raster" | "webgpu";
/** Compiler cache value; retain the root and all code parts together. */
export interface CompiledProgram {
  module: WebAssembly.Module;
  parts: WebAssembly.Module[];
}
export interface SessionOptions {
  /** Defaults to the shipped classic worker next to session.js. */
  workerUrl?: string | URL;
  runtime: Binary | WebAssembly.Module;
  program: Binary;
  assets: { path: string; bytes: Binary }[];
  graphicsProfile: GraphicsProfile;
  audioEnabled?: boolean;
  forceInterpreter?: boolean;
  motionAvailability?: 0 | 1 | 2;
  pointerCaptureSupported?: boolean;
  mediatedInputKinds?: string[];
  /** Required for either WebGPU profile, using the runtime's GPU wire format. */
  gpuCapabilities?: ArrayBuffer;
  cacheKey?: string;
  compiledBytes?: ArrayBuffer;
  compiledProgram?: CompiledProgram;
  /** Ordered worker output, including output during initialization. A throw ends the session. */
  onOutput: (output: RuntimeOutput) => void;
}

/** Wire records use the current runtime ABI; this library does not grant host permissions. */
export type RuntimeInput =
  | { type: "input"; bytes: Binary }
  | { type: "view-insets"; eventType: 16 | 17; left: number; top: number; right: number; bottom: number }
  | { type: "pause"; paused: boolean }
  | { type: "background"; backgrounded: boolean; seq?: number }
  | { type: "motion-status"; availability: 0 | 1 | 2 }
  | { type: "pointer-capture-support"; supported: boolean }
  | { type: "pointer-capture-state"; active: boolean }
  | { type: "motion" | "gpu-capabilities" | "gpu-event"; bytes: Binary }
  | { type: "host-frame-response"; bytes: Binary; seq?: number }
  | { type: "mediated-input-result"; handle: number; status: 3 | 4 | 5 | 6; bytes: Binary };

export interface ReadyOutput {
  type: "ready";
  backend: "interpreter" | "compiler";
  compilerFallbackReason?: string;
  compilerFallbackStage?: string;
  usesMotion: boolean;
  usesPointerCapture: boolean;
  usesUpdateScheduling: boolean;
  cacheHit: boolean;
  translationMs: number;
  compilationMs: number;
  translatedWasmBytes: number;
  startupMs: number;
}
export type UiCommand =
  | { type: "copy-text"; text: string }
  | { type: "open-url"; url: string; newSurface: boolean }
  | { type: "copy-image"; width: number; height: number; rgba: Uint8Array };
export interface UiOutput {
  cursorIcon: string;
  mutableTextUnderCursor: boolean;
  ime: { rect: number[]; cursorRect: number[] } | null;
  commands: UiCommand[];
}
export type RuntimeOutput =
  | ReadyOutput
  | { type: "startup"; stage: string }
  | { type: "frame"; width: number; height: number; pixels: Uint8Array }
  | { type: "tri2d" | "ui-semantics" | "gpu-batch" | "host-frame-request" | "save"; bytes: Uint8Array }
  | { type: "ui-output"; output: UiOutput }
  | { type: "audio"; sampleRate: number; channels: number; samples: Uint8Array }
  | { type: "log"; message: string }
  | { type: "pointer-capture"; capture: boolean }
  | { type: "pause-state"; paused: boolean }
  | { type: "background-state"; backgrounded: boolean; seq?: number }
  | { type: "host-frame-response-accepted"; seq: number }
  | { type: "host-frame-response-rejected"; reason: "queue-full"; seq?: number }
  | { type: "mediated-input-request"; handle: number; kind: string; mediaType: string; maxBytes: number }
  | { type: "mediated-input-cancel"; handle: number }
  | { type: "translated"; cacheKey?: string; bytes: Uint8Array }
  | { type: "compiled"; cacheKey?: string; program: CompiledProgram }
  | { type: "metrics"; updates: number; updateP50Ms: number; updateP95Ms: number; updateMaxMs: number }
  | { type: "error"; message: string }
  | { type: "terminated" };
export type SessionTerminal =
  | { reason: "stopped" | "terminated" }
  | { reason: "error"; error: Error };
export interface BrowserSession {
  readonly state: "starting" | "running" | "stopping" | "terminated";
  /** Rejects on startup error or stop-before-ready (AbortError). */
  readonly ready: Promise<ReadyOutput>;
  /** Always resolves, after listeners/timers are removed and the Worker is terminated. */
  readonly terminal: Promise<SessionTerminal>;
  /** After ready only. Binary buffers are cloned; the caller retains ownership. */
  send(input: RuntimeInput): void;
  /** Idempotent. Requests cancellation, forcibly terminates after at most 1s of host scheduling. */
  stop(): Promise<SessionTerminal>;
}
export function startSession(options: SessionOptions): BrowserSession;
