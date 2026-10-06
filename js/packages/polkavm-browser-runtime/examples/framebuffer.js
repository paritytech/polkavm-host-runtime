/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

import { startSession } from "../dist/session.js";

const canvas = document.querySelector("canvas");
const context = canvas.getContext("2d");
const status = document.querySelector("#status");
const pause = document.querySelector("#pause");
const background = document.querySelector("#background");
const stop = document.querySelector("#stop");
let session;
let paused = false;
let backgrounded = false;
let frames = 0;
let backend = "starting";

async function bytes(url) {
  const response = await fetch(new URL(url, import.meta.url));
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.arrayBuffer();
}

function showStatus() {
  status.textContent = `${backend}; ${frames} frame(s); paused=${paused}; backgrounded=${backgrounded}`;
}

pause.addEventListener("click", () => {
  paused = !paused;
  session.send({ type: "pause", paused });
  pause.textContent = paused ? "Resume" : "Pause";
});
background.addEventListener("click", () => {
  backgrounded = !backgrounded;
  session.send({ type: "background", backgrounded });
  background.textContent = backgrounded ? "Foreground" : "Background";
});
stop.addEventListener("click", () => { void session.stop(); });
window.addEventListener("pagehide", () => { void session?.stop(); }, { once: true });

try {
  const [runtimeBytes, program] = await Promise.all([
    bytes("../dist/polkavm-browser-runtime.wasm"),
    bytes("../../../../rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm"),
  ]);
  session = startSession({
    runtime: await WebAssembly.compile(runtimeBytes),
    program,
    assets: [],
    graphicsProfile: "framebuffer",
    audioEnabled: false,
    onOutput(output) {
      if (output.type === "frame") {
        canvas.width = output.width;
        canvas.height = output.height;
        context.putImageData(new ImageData(new Uint8ClampedArray(
          output.pixels.buffer, output.pixels.byteOffset, output.pixels.byteLength,
        ), output.width, output.height), 0, 0);
        frames++;
        showStatus();
      } else if (output.type === "pause-state" || output.type === "background-state") {
        showStatus();
      }
    },
  });
  void session.terminal.then((result) => {
    pause.disabled = background.disabled = stop.disabled = true;
    status.textContent = result.reason === "error"
      ? `Worker released: ${result.error.message}`
      : `Worker released: ${result.reason}; rendered ${frames} frame(s)`;
  });
  const ready = await session.ready;
  backend = ready.backend;
  pause.disabled = background.disabled = stop.disabled = false;
  showStatus();
} catch (error) {
  status.textContent = `Failed: ${error.message}`;
  await session?.stop();
}
