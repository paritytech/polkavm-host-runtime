import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
if (process.argv.length !== 4 || process.argv[2] !== "--output") {
  throw new Error("usage: create-release-manifest.mjs --output <path>");
}

const git = (...args) =>
  execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
if (git("status", "--porcelain=v1", "--untracked-files=all")) {
  throw new Error("release artifacts require a clean source checkout");
}
const sourceRevision = git("rev-parse", "HEAD");
if (!/^[0-9a-f]{40}$/.test(sourceRevision)) {
  throw new Error("git did not return an immutable source revision");
}

const packageJson = JSON.parse(
  await readFile(resolve(root, "package.json"), "utf8"),
);
const browserRoot = resolve(root, "js/packages/polkavm-browser-runtime");
const browserPackage = JSON.parse(
  await readFile(resolve(browserRoot, "package.json"), "utf8"),
);
const lock = JSON.parse(await readFile(resolve(root, "package-lock.json"), "utf8"));
const version = packageJson.version;
const versions = [
  browserPackage.version,
  lock.version,
  lock.packages?.[""]?.version,
  lock.packages?.["js/packages/polkavm-browser-runtime"]?.version,
];
const workspaceToml = await readFile(resolve(root, "Cargo.toml"), "utf8");
const workspacePackage = workspaceToml.match(
  /^\[workspace\.package\][ \t]*\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m,
);
const workspaceVersion = workspacePackage?.[1].match(
  /^version\s*=\s*"([^"]+)"\s*$/m,
)?.[1];
versions.push(workspaceVersion);
const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"], {
    cwd: root,
    encoding: "utf8",
  }),
);
const members = metadata.packages.filter((pkg) =>
  metadata.workspace_members.includes(pkg.id),
);
versions.push(...members.map((pkg) => pkg.version));
if (typeof version !== "string" || versions.some((value) => value !== version)) {
  throw new Error("workspace, crate, browser package, and npm lockfile versions must match");
}
const tag = `v${version}`;
if (git("rev-parse", "--verify", `refs/tags/${tag}^{commit}`) !== sourceRevision) {
  throw new Error(`release source must be the commit tagged ${tag}`);
}

const source = await readFile(resolve(browserRoot, "SOURCE"), "utf8");
if (!source.split(/\r?\n/).includes(`Release tag: ${tag}`)) {
  throw new Error("SOURCE release tag does not match the package version");
}
const runtime = members.find((pkg) => pkg.name === "polkavm-host-runtime");
const compiler = members.find((pkg) => pkg.name === "polkavm-wasm-compiler");
if (!runtime || !compiler) throw new Error("workspace is missing the runtime or compiler");
const pinnedRevision = (dependency) => {
  const pinned = dependency?.source?.startsWith("git+")
    ? new URL(dependency.source.slice(4)).searchParams.get("rev")
    : null;
  if (!pinned || !/^[0-9a-f]{40}$/.test(pinned)) {
    throw new Error("PolkaVM dependencies require full immutable Git revisions");
  }
  return pinned;
};
const engine = runtime.dependencies.find(
  (dep) => dep.name === "polkavm" && dep.rename === null && dep.target === null && dep.kind === null,
);
const engineRevision = pinnedRevision(engine);
const revision = (label) => {
  const matches = [...source.matchAll(new RegExp(`^${label}: ([0-9a-f]{40})$`, "gm"))];
  if (matches.length !== 1 || matches[0][1] !== engineRevision) {
    throw new Error(`SOURCE ${label} does not match the pinned Cargo dependency`);
  }
  return engineRevision;
};
const nativeRevision = revision("PolkaVM native revision");
const wasmRevision = revision("PolkaVM wasm revision");
const compilerCommon = compiler.dependencies.find(
  (dep) => dep.name === "polkavm-common" && dep.kind === null,
);
if (pinnedRevision(compilerCommon) !== engineRevision) {
  throw new Error("compiler polkavm-common revision differs from the runtime engine");
}
for (const pkg of members) {
  for (const dependency of pkg.dependencies) {
    if (["polkavm", "polkavm-common"].includes(dependency.name) && pinnedRevision(dependency) !== engineRevision) {
      throw new Error(`${pkg.name} ${dependency.name} revision differs from the runtime engine`);
    }
  }
}

const dist = resolve(browserRoot, "dist");
const embedded = resolve(root, "rust/crates/polkavm-host-runtime-assets/assets");
for (const directory of [dist, embedded]) {
  if (!(await lstat(directory)).isDirectory()) {
    throw new Error(`artifact directory is not a regular directory: ${directory}`);
  }
  if (!(await lstat(resolve(directory, "SHA256SUMS"))).isFile()) {
    throw new Error("artifact checksums are not a regular file");
  }
}
const sums = await readFile(resolve(dist, "SHA256SUMS"), "utf8");
const artifacts = {};
for (const line of sums.trimEnd().split("\n")) {
  const match = /^([0-9a-f]{64})  ([A-Za-z0-9][A-Za-z0-9._-]*)$/.exec(line);
  if (!match) throw new Error("invalid browser checksum record");
  const [, sha256, file] = match;
  if (file === "SHA256SUMS" || Object.hasOwn(artifacts, file)) {
    throw new Error(`duplicate or reserved checksum path: ${file}`);
  }
  const path = resolve(dist, file);
  if (!(await lstat(path)).isFile()) {
    throw new Error(`browser artifact is not a regular file: ${file}`);
  }
  const bytes = await readFile(path);
  if (createHash("sha256").update(bytes).digest("hex") !== sha256) {
    throw new Error(`browser artifact checksum mismatch: ${file}`);
  }
  artifacts[file] = { sha256, size: bytes.length };
}
const files = [...Object.keys(artifacts), "SHA256SUMS"].sort();
for (const directory of [dist, embedded]) {
  if (JSON.stringify((await readdir(directory)).sort()) !== JSON.stringify(files)) {
    throw new Error(`browser artifact inventory differs from checksums: ${directory}`);
  }
}
for (const file of files) {
  if (!(await lstat(resolve(embedded, file))).isFile()) {
    throw new Error(`embedded artifact is not a regular file: ${file}`);
  }
  if (!(await readFile(resolve(dist, file))).equals(await readFile(resolve(embedded, file)))) {
    throw new Error(`browser artifact differs from embedded release bytes: ${file}`);
  }
}

const manifest = {
  schemaVersion: 1,
  version,
  sourceRepository: "https://github.com/paritytech/polkavm-host-runtime",
  sourceRevision,
  rustCrates: Object.fromEntries(members.map((pkg) => [pkg.name, pkg.version])),
  npmPackages: {
    [browserPackage.name]: browserPackage.version,
  },
  polkavm: { nativeRevision, wasmRevision },
  artifacts,
};
const outputPath = resolve(root, process.argv[3]);
await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
