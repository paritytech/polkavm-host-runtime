import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const dist = resolve(root, "js/packages/pvm-browser-runtime/dist");
const embedded = resolve(root, "rust/crates/pvm-runtime-assets/assets");

const distFiles = (await readdir(dist)).sort();
const embeddedFiles = (await readdir(embedded)).sort();
if (JSON.stringify(distFiles) !== JSON.stringify(embeddedFiles)) {
  throw new Error(
    `browser asset inventory mismatch\ndist: ${distFiles.join(", ")}\nembedded: ${embeddedFiles.join(", ")}`,
  );
}

for (const file of distFiles) {
  const generated = await readFile(resolve(dist, file));
  const packaged = await readFile(resolve(embedded, file));
  if (!generated.equals(packaged)) {
    throw new Error(`browser asset differs from source build: ${file}`);
  }
  console.log(
    `${createHash("sha256").update(generated).digest("hex")}  ${file}`,
  );
}
