import { attachFileInputControls } from "../src/file-input-router.js";

const supafaust = {
  id: "supafaust",
  displayName: "Supafaust",
  manifest: {
    runtime: { kind: "polkavm", abiVersion: 1, entrypoint: "app.polkavm" },
    capabilities: {
      fileInput: {
        abiVersion: 1,
        handlers: [
          {
            id: "snes-rom",
            label: "SNES cartridge image",
            extensions: [".sfc", ".smc", ".swc", ".fig"],
            maxBytes: 16 * 1024 * 1024,
            mountPath: "game/cartridge.sfc",
          },
        ],
      },
    },
  },
};

const dropTarget = document.querySelector("#drop-zone");
const openButton = document.querySelector("#open-file");
const result = document.querySelector("#result");
const dialog = document.querySelector("#consent-dialog");
const details = document.querySelector("#file-details");
const confirmButton = document.querySelector("#confirm-delivery");
const cancelButton = document.querySelector("#cancel-delivery");

function askForConsent({ file, product, handler }) {
  details.textContent = `${file.name} · ${(file.size / 1024).toFixed(1)} KiB\n${handler.label} → ${product.displayName}`;
  dialog.showModal();
  return new Promise((resolve) => {
    const finish = (approved) => {
      dialog.close();
      confirmButton.onclick = null;
      cancelButton.onclick = null;
      resolve(approved);
    };
    confirmButton.onclick = () => finish(true);
    cancelButton.onclick = () => finish(false);
    dialog.oncancel = (event) => {
      event.preventDefault();
      finish(false);
    };
  });
}

attachFileInputControls({
  products: [supafaust],
  dropTarget,
  openButton,
  confirmDelivery: askForConsent,
  launchProduct({ product, handler, asset }) {
    // This prototype demonstrates the handoff only; no guest is launched.
    const heading = document.createElement("strong");
    heading.textContent = `Simulated delivery to ${product.displayName}`;
    result.replaceChildren(
      heading,
      document.createElement("br"),
      document.createTextNode(
        `${asset.bytes.byteLength.toLocaleString()} bytes read for ${handler.mountPath}. No emulator was launched or cartridge validated.`,
      ),
    );
  },
  onResult(delivery) {
    if (delivery.status === "unhandled") {
      result.textContent = "No installed product registered a handler for that file.";
    } else if (delivery.status === "declined" || delivery.status === "cancelled") {
      result.textContent = "Delivery cancelled. The file was not read.";
    } else if (delivery.status === "rejected") {
      result.textContent = `File delivery failed: ${delivery.error?.message ?? "unknown error"}`;
    }
  },
});

for (const event of ["dragenter", "dragover"]) {
  dropTarget.addEventListener(event, () => dropTarget.classList.add("dragging"));
}
for (const event of ["dragleave", "drop"]) {
  dropTarget.addEventListener(event, () => dropTarget.classList.remove("dragging"));
}
