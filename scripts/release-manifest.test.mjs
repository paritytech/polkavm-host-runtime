import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

const nativeRevision = "1".repeat(40);
const wasmRevision = "2".repeat(40);
const browserPath = "js/packages/pvm-browser-runtime";

async function fixture(t, prepare = async () => {}) {
  const root = await mkdtemp(join(tmpdir(), "pvm-release-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const put = async (path, value) => {
    await mkdir(dirname(join(root, path)), { recursive: true });
    await writeFile(join(root, path), value);
  };
  const git = (...args) => execFileSync("git", [
    "-c", "user.name=Release test", "-c", "user.email=release@example.invalid",
    "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false",
    "-c", "core.hooksPath=/dev/null", ...args,
  ], { cwd: root, encoding: "utf8" }).trim();
  await put(".gitignore", "/artifacts/\n/js/packages/pvm-browser-runtime/dist/\n/target/\n");
  await put("package.json", JSON.stringify({ version: "0.1.0", type: "module" }));
  await put(`${browserPath}/package.json`, JSON.stringify({ name: "@parity/pvm-browser-runtime", version: "0.1.0" }));
  await put("package-lock.json", JSON.stringify({ version: "0.1.0", packages: {
    "": { version: "0.1.0" }, [browserPath]: { version: "0.1.0" },
  } }));
  await put("Cargo.toml", `[workspace]\nresolver = "2"\nmembers = ["runtime"]\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2021"\n`);
  await put("runtime/Cargo.toml", `[package]\nname = "pvm-runtime"\nversion = "0.1.0"\nedition = "2021"\n[target.'cfg(not(target_arch = "wasm32"))'.dependencies]\npolkavm = { git = "https://github.com/paritytech/polkavm.git", rev = "${nativeRevision}" }\n[target.'cfg(target_arch = "wasm32")'.dependencies]\npolkavm-wasm = { package = "polkavm", git = "https://github.com/paritytech/polkavm.git", rev = "${wasmRevision}" }\n`);
  await put("runtime/src/lib.rs", "//! Release fixture.\n");
  await put(`${browserPath}/SOURCE`, `Release tag: v0.1.0\nPolkaVM native revision: ${nativeRevision}\nPolkaVM wasm revision: ${wasmRevision}\n`);
  const bytes = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
  const digest = createHash("sha256").update(bytes).digest("hex");
  for (const directory of [`${browserPath}/dist`, "rust/crates/pvm-runtime-assets/assets"]) {
    await put(`${directory}/runtime.wasm`, bytes);
    await put(`${directory}/SHA256SUMS`, `${digest}  runtime.wasm\n`);
  }
  await mkdir(join(root, "scripts"));
  await copyFile(new URL("./create-release-manifest.mjs", import.meta.url), join(root, "scripts/create-release-manifest.mjs"));
  await prepare({ root, put });
  git("init", "--quiet");
  git("add", ".");
  git("commit", "--quiet", "-m", "Release fixture");
  git("tag", "v0.1.0");
  const run = () => spawnSync(process.execPath, ["scripts/create-release-manifest.mjs", "--output", "artifacts/release.json"], {
    cwd: root, encoding: "utf8", timeout: 30_000,
  });
  return { root, put, git, run, digest, bytes };
}

async function rejected(f, pattern) {
  const result = f.run();
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, pattern);
  await assert.rejects(readFile(join(f.root, "artifacts/release.json")), { code: "ENOENT" });
}

test("release manifest binds clean tagged source to verified artifact bytes", async (t) => {
  const f = await fixture(t);
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  const manifest = JSON.parse(await readFile(join(f.root, "artifacts/release.json"), "utf8"));
  assert.equal(manifest.sourceRevision, f.git("rev-parse", "HEAD"));
  assert.deepEqual(manifest.artifacts["runtime.wasm"], { sha256: f.digest, size: f.bytes.length });
  assert.deepEqual(manifest.rustCrates, { "pvm-runtime": "0.1.0" });
  assert.deepEqual(manifest.polkavm, { nativeRevision, wasmRevision });
});

for (const mode of ["unstaged", "staged", "untracked"]) {
  test(`release refuses ${mode} source changes`, async (t) => {
    const f = await fixture(t);
    await f.put(mode === "untracked" ? "new-source.rs" : "runtime/src/lib.rs", "// changed source\n");
    if (mode === "staged") f.git("add", "runtime/src/lib.rs");
    await rejected(f, /clean source checkout/);
  });
}

test("release refuses a clean commit that is not the version tag", async (t) => {
  const f = await fixture(t);
  f.git("commit", "--quiet", "--allow-empty", "-m", "After release");
  await rejected(f, /commit tagged v0.1.0/);
});

test("release refuses a package version inconsistent with the workspace", async (t) => {
  const f = await fixture(t, ({ put }) => put(`${browserPath}/package.json`, JSON.stringify({ name: "@parity/pvm-browser-runtime", version: "0.2.0" })));
  await rejected(f, /versions must match/);
});

test("release refuses provenance inconsistent with Cargo dependency revisions", async (t) => {
  const f = await fixture(t, ({ put }) => put(`${browserPath}/SOURCE`, `Release tag: v0.1.0\nPolkaVM native revision: ${wasmRevision}\nPolkaVM wasm revision: ${wasmRevision}\n`));
  await rejected(f, /does not match the pinned Cargo dependency/);
});

test("release refuses modified generated artifact bytes", async (t) => {
  const f = await fixture(t);
  await f.put(`${browserPath}/dist/runtime.wasm`, "tampered");
  await rejected(f, /checksum mismatch/);
});

test("release refuses artifacts omitted from the checksum inventory", async (t) => {
  const f = await fixture(t);
  await f.put(`${browserPath}/dist/extra.js`, "unexpected");
  await rejected(f, /inventory differs/);
});

test("release refuses generated bytes that differ from embedded assets", async (t) => {
  const f = await fixture(t, ({ put }) => put("rust/crates/pvm-runtime-assets/assets/runtime.wasm", "stale"));
  await rejected(f, /differs from embedded release bytes/);
});

test("release refuses checksum paths outside the artifact directory", async (t) => {
  const f = await fixture(t);
  await f.put(`${browserPath}/dist/SHA256SUMS`, `${f.digest}  ../runtime.wasm\n`);
  await rejected(f, /invalid browser checksum record/);
});
