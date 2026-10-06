import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

const engineRevision = "1".repeat(40);
const otherRevision = "2".repeat(40);
const browserPath = "js/packages/polkavm-browser-runtime";
const embeddedPath = "rust/crates/polkavm-host-runtime-assets/assets";
const crateNames = [
  "polkavm-host-runtime", "polkavm-wasm-compiler", "polkavm-host-runtime-assets",
  "polkavm-gpu-wire", "polkavm-motion-wire", "polkavm-ui-wire", "polkavm-assets-export",
];
const dependency = (revision = engineRevision) =>
  `{ git = "https://github.com/paritytech/polkavm.git", rev = "${revision}" }`;
const crateManifest = (name, dependencies = "", version = "0.1.0") =>
  `[package]\nname = "${name}"\nversion = "${version}"\nedition = "2021"\n${dependencies}`;
const source = (native = engineRevision, wasm = engineRevision) =>
  `Release tag: v0.1.0\nPolkaVM native revision: ${native}\nPolkaVM wasm revision: ${wasm}\n`;

async function fixture(t, prepare = async () => {}) {
  const root = await mkdtemp(join(tmpdir(), "polkavm-release-"));
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
  await put(".gitignore", `/artifacts/\n/${browserPath}/dist/\n/target/\n`);
  await put("package.json", JSON.stringify({ version: "0.1.0", type: "module" }));
  await put(`${browserPath}/package.json`, JSON.stringify({ name: "@parity/polkavm-browser-runtime", version: "0.1.0" }));
  await put("package-lock.json", JSON.stringify({ version: "0.1.0", packages: {
    "": { version: "0.1.0" }, [browserPath]: { version: "0.1.0" },
  } }));
  await put("Cargo.toml", `[workspace]\nresolver = "2"\nmembers = ${JSON.stringify(crateNames)}\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2021"\n`);
  for (const name of crateNames) {
    let dependencies = "";
    if (name === "polkavm-host-runtime") {
      dependencies = `[dependencies]\npolkavm = ${dependency()}\n[target.'cfg(target_arch = "wasm32")'.dependencies]\npolkavm-common = ${dependency()}\n`;
    } else if (name === "polkavm-wasm-compiler") {
      dependencies = `[dependencies]\npolkavm-common = ${dependency()}\n[dev-dependencies]\npolkavm = ${dependency()}\n`;
    }
    await put(`${name}/Cargo.toml`, crateManifest(name, dependencies));
    await put(`${name}/src/lib.rs`, "//! Release fixture.\n");
  }
  await put(`${browserPath}/SOURCE`, source());
  const bytes = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
  const digest = createHash("sha256").update(bytes).digest("hex");
  for (const directory of [`${browserPath}/dist`, embeddedPath]) {
    await put(`${directory}/runtime.wasm`, bytes);
    await put(`${directory}/SHA256SUMS`, `${digest}  runtime.wasm\n`);
  }
  await mkdir(join(root, "scripts"));
  await copyFile(new URL("./create-release-manifest.mjs", import.meta.url), join(root, "scripts/create-release-manifest.mjs"));
  await prepare({ root, put, digest });
  git("init", "--quiet");
  git("add", ".");
  git("commit", "--quiet", "-m", "Release fixture");
  git("tag", "v0.1.0");
  const run = () => spawnSync(process.execPath, ["scripts/create-release-manifest.mjs", "--output", "artifacts/release.json"], {
    cwd: root, encoding: "utf8", timeout: 30_000,
  });
  return { root, put, git, run, digest, bytes };
}

async function rejected(f) {
  const result = f.run();
  assert.equal(result.status, 1, result.stderr);
  await assert.rejects(readFile(join(f.root, "artifacts/release.json")), { code: "ENOENT" });
}

test("release manifest binds clean tagged source to verified artifact bytes and all workspace crates", async (t) => {
  const f = await fixture(t);
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  const manifest = JSON.parse(await readFile(join(f.root, "artifacts/release.json"), "utf8"));
  assert.equal(manifest.sourceRevision, f.git("rev-parse", "HEAD"));
  assert.equal(manifest.version, "0.1.0");
  assert.deepEqual(manifest.artifacts, { "runtime.wasm": { sha256: f.digest, size: f.bytes.length } });
  assert.deepEqual(manifest.rustCrates, Object.fromEntries(crateNames.map((name) => [name, "0.1.0"])));
  assert.deepEqual(manifest.npmPackages, { "@parity/polkavm-browser-runtime": "0.1.0" });
  assert.deepEqual(manifest.polkavm, { nativeRevision: engineRevision, wasmRevision: engineRevision });
});

for (const mode of ["unstaged", "staged", "untracked"]) {
  test(`release refuses ${mode} source changes`, async (t) => {
    const f = await fixture(t);
    const path = mode === "untracked" ? "new-source.rs" : "polkavm-host-runtime/src/lib.rs";
    await f.put(path, "// changed source\n");
    if (mode === "staged") f.git("add", path);
    await rejected(f);
  });
}

test("release refuses a clean commit that is not the version tag", async (t) => {
  const f = await fixture(t);
  f.git("commit", "--quiet", "--allow-empty", "-m", "After release");
  await rejected(f);
});

test("release refuses a package version inconsistent with the workspace", async (t) => {
  const f = await fixture(t, ({ put }) => put(`${browserPath}/package.json`, JSON.stringify({ name: "@parity/polkavm-browser-runtime", version: "0.2.0" })));
  await rejected(f);
});

for (const name of crateNames) {
  test(`release refuses an inconsistent ${name} version`, async (t) => {
    const f = await fixture(t, async ({ root, put }) => {
      const path = `${name}/Cargo.toml`;
      await put(path, (await readFile(join(root, path), "utf8")).replace('version = "0.1.0"', 'version = "0.2.0"'));
    });
    await rejected(f);
  });
}

for (const label of ["native", "wasm"]) {
  test(`release refuses ${label} provenance inconsistent with the unified engine`, async (t) => {
    const f = await fixture(t, ({ put }) => put(`${browserPath}/SOURCE`, label === "native" ? source(otherRevision) : source(engineRevision, otherRevision)));
    await rejected(f);
  });
}

for (const [condition, name, dependencies] of [
  ["mismatched compiler common pin", "polkavm-wasm-compiler", `[dependencies]\npolkavm-common = ${dependency(otherRevision)}\n`],
  ["abbreviated compiler common pin", "polkavm-wasm-compiler", `[dependencies]\npolkavm-common = ${dependency(engineRevision.slice(0, 8))}\n`],
  ["missing compiler common pin", "polkavm-wasm-compiler", ""],
  ["mismatched compiler engine pin", "polkavm-wasm-compiler", `[dependencies]\npolkavm-common = ${dependency()}\n[dev-dependencies]\npolkavm = ${dependency(otherRevision)}\n`],
  ["mismatched runtime common pin", "polkavm-host-runtime", `[dependencies]\npolkavm = ${dependency()}\n[target.'cfg(target_arch = "wasm32")'.dependencies]\npolkavm-common = ${dependency(otherRevision)}\n`],
]) {
  test(`release refuses ${condition}`, async (t) => {
    const f = await fixture(t, ({ put }) => put(`${name}/Cargo.toml`, crateManifest(name, dependencies)));
    await rejected(f);
  });
}

test("release refuses development provenance even on a version-tagged commit", async (t) => {
  const f = await fixture(t, ({ put }) => put(`${browserPath}/SOURCE`, source().replace("Release tag: v0.1.0", "Development package: 0.1.0")));
  await rejected(f);
});

test("release refuses modified generated artifact bytes", async (t) => {
  const f = await fixture(t);
  await f.put(`${browserPath}/dist/runtime.wasm`, "tampered");
  await rejected(f);
});

for (const directory of [`${browserPath}/dist`, embeddedPath]) {
  test(`release refuses unlisted files in ${directory}`, async (t) => {
    const f = await fixture(t, ({ put }) => put(`${directory}/extra.js`, "unexpected"));
    await rejected(f);
  });
  for (const file of ["runtime.wasm", "SHA256SUMS"]) {
    test(`release refuses symlinked ${directory}/${file}`, async (t) => {
      const f = await fixture(t, async ({ root, put }) => {
        const path = join(root, directory, file);
        await put(`artifacts/${file}`, await readFile(path));
        await rm(path);
        await symlink(join(root, "artifacts", file), path);
      });
      await rejected(f);
    });
  }
}

test("release refuses generated bytes that differ from embedded assets", async (t) => {
  const f = await fixture(t, ({ put }) => put(`${embeddedPath}/runtime.wasm`, "stale"));
  await rejected(f);
});

for (const path of ["../runtime.wasm", "/runtime.wasm", "nested/runtime.wasm", "SHA256SUMS"]) {
  test(`release refuses unsafe checksum path ${path}`, async (t) => {
    const f = await fixture(t);
    await f.put(`${browserPath}/dist/SHA256SUMS`, `${f.digest}  ${path}\n`);
    await rejected(f);
  });
}

test("release refuses duplicate checksum records", async (t) => {
  const f = await fixture(t);
  await f.put(`${browserPath}/dist/SHA256SUMS`, `${f.digest}  runtime.wasm\n`.repeat(2));
  await rejected(f);
});
