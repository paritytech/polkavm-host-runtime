/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, readdir, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const check = process.argv.includes("--check");
if (process.argv.slice(2).some((arg) => arg !== "--check")) {
  throw new Error("Usage: node scripts/generate-third-party-notices.mjs [--check]");
}
const hash = (text) => createHash("sha256").update(text).digest("hex");
const command = (bin, args) => execFileSync(bin, args, {
  cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024,
});
const metadata = (args) => JSON.parse(command("cargo", [
  "metadata", "--locked", "--offline", "--format-version", "1", ...args,
]));
const all = metadata(["--all-features"]);
const browser = metadata(["--filter-platform", "wasm32-unknown-unknown"]);
const packages = new Map(all.packages.map((pkg) => [pkg.id, pkg]));
const membership = new Map();

// A build edge or proc macro makes its entire subtree build-time, unless that
// package is independently reached by a normal runtime path as well.
function collect(graph, roots, scope) {
  const nodes = new Map(graph.resolve.nodes.map((node) => [node.id, node]));
  const seen = new Set();
  function visit(id, kind) {
    const pkg = packages.get(id);
    if (pkg.targets.some((target) => target.kind.includes("proc-macro"))) kind = "build";
    const key = `${id}\n${kind}`;
    if (seen.has(key)) return;
    seen.add(key);
    if (pkg.source) {
      if (!membership.has(id)) membership.set(id, new Set());
      membership.get(id).add(`${scope}/${kind}`);
    }
    for (const dep of nodes.get(id)?.deps ?? []) {
      for (const edge of dep.dep_kinds) {
        if (edge.kind === "dev") continue;
        visit(dep.pkg, kind === "build" || edge.kind === "build" ? "build" : "runtime");
      }
    }
  }
  for (const id of roots) visit(id, "runtime");
}
collect(all, all.workspace_members, "all-targets-all-features");
const browserRoot = browser.packages.find((pkg) => pkg.name === "polkavm-host-runtime" && !pkg.source);
if (!browserRoot) throw new Error("Browser runtime workspace root missing");
collect(browser, [browserRoot.id], "browser-default");

const lockText = await readFile(join(root, "Cargo.lock"), "utf8");
const locked = lockText.split("[[package]]").slice(1).map((block) => {
  const result = {};
  for (const key of ["name", "version", "source", "checksum"]) {
    result[key] = block.match(new RegExp(`^${key} = "([^"\\n]+)"`, "m"))?.[1] ?? null;
  }
  return result;
});
const supplements = JSON.parse(await readFile(join(root, "licenses/upstream-supplements.json"), "utf8"));
for (const item of supplements) {
  if (hash(item.text) !== item.sha256) throw new Error(`Supplement changed: ${item.source}`);
}
const documents = new Map();
function addDocument(text, owner, source, note) {
  const digest = hash(text);
  if (!documents.has(digest)) documents.set(digest, { text, uses: new Map() });
  const usage = { owner, source, ...(note ? { note } : {}) };
  documents.get(digest).uses.set(JSON.stringify(usage), usage);
  return digest;
}
const legalName = /^(?:licen[sc]es?|copying|copyright|notice|unlicense|authors)(?:[._-]|$)/i;
const excluded = new Set([".git", "target", "tests", "test", "examples", "benches", "fuzz"]);
async function legalFiles(dir, prefix = "", inLegalDirectory = false) {
  const result = [];
  for (const entry of (await readdir(dir, { withFileTypes: true })).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
    const name = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory() && !excluded.has(entry.name)) {
      result.push(...await legalFiles(join(dir, entry.name), name, inLegalDirectory || legalName.test(entry.name)));
    } else if (entry.isFile() && (inLegalDirectory || legalName.test(entry.name))) {
      result.push(name);
    }
  }
  return result;
}
async function sourceHeaders(dir, prefix = "") {
  const result = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const name = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory() && !excluded.has(entry.name)) {
      result.push(...await sourceHeaders(join(dir, entry.name), name));
    } else if (entry.isFile() && /\.(?:rs|c|h|cpp|hpp|xml|wgsl|idl|S|s|asm)$/.test(entry.name)) {
      const text = await readFile(join(dir, entry.name), "utf8");
      // Preserve actual upstream notice blocks, including Khronos/generated
      // source headers. These are not inferred from Cargo's authors field.
      for (const match of text.matchAll(/\/\*[\s\S]*?\*\/|<!--[\s\S]*?-->|<comment>[\s\S]*?<\/comment>|(?:^[ \t]*\/\/[^\n]*(?:\n|$))+/gm)) {
        if (/copyright|permission is hereby granted|licensed under|SPDX-License-Identifier/i.test(match[0])) {
          result.push({ name, text: match[0] });
        }
      }
    }
  }
  return result.sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
}
const inventory = [];
for (const id of [...membership.keys()].sort()) {
  const pkg = packages.get(id);
  if (!pkg.source.startsWith("registry+https://github.com/rust-lang/crates.io-index") &&
      !pkg.source.startsWith("git+https://github.com/paritytech/polkavm.git")) {
    throw new Error(`Review non-public or unrecognized dependency source for ${pkg.name}`);
  }
  const owner = `${pkg.name}@${pkg.version}`;
  const lock = locked.find((entry) => entry.name === pkg.name && entry.version === pkg.version && entry.source === pkg.source);
  if (!lock) throw new Error(`No exact Cargo.lock entry: ${owner}`);
  const dir = dirname(pkg.manifest_path);
  const hashes = new Set();
  let legalCount = 0;
  for (const file of await legalFiles(dir)) {
    hashes.add(addDocument(await readFile(join(dir, file), "utf8"), owner, `${pkg.source}; ${file}`));
    legalCount++;
  }
  if (pkg.license_file) {
    hashes.add(addDocument(await readFile(resolve(dir, pkg.license_file), "utf8"), owner, `${pkg.source}; ${pkg.license_file}`));
    legalCount++;
  }
  if (pkg.source.startsWith("git+")) {
    // Cargo's git package is a subdirectory; preserve its repository notices.
    let ancestor = dirname(dir);
    while (ancestor !== dirname(ancestor)) {
      const entries = await readdir(ancestor, { withFileTypes: true });
      for (const entry of entries.filter((item) => item.isFile() && legalName.test(item.name))) {
        hashes.add(addDocument(await readFile(join(ancestor, entry.name), "utf8"), owner,
          `${pkg.source}; ${relative(dir, join(ancestor, entry.name)).replaceAll("\\", "/")}`));
        legalCount++;
      }
      if (entries.some((item) => item.name === ".git")) break;
      ancestor = dirname(ancestor);
    }
  }
  for (const item of supplements.filter((item) => item.packages.includes(owner))) {
    hashes.add(addDocument(item.text, owner, item.source, item.note));
    legalCount++;
  }
  if (!legalCount) throw new Error(`No license text for ${owner}; review and add a pinned upstream supplement`);
  for (const item of await sourceHeaders(dir)) {
    hashes.add(addDocument(item.text, owner, `${pkg.source}; ${item.name} (source notice)`));
  }
  inventory.push({ name: pkg.name, version: pkg.version, source: pkg.source,
    checksum: lock.checksum, license: pkg.license, authors: pkg.authors,
    scopes: [...membership.get(id)].sort(), documents: [...hashes].sort() });
}
for (const item of supplements.filter((item) => item.packages.includes("vendored-polkavm-examples"))) {
  addDocument(item.text, "vendored-polkavm-examples", item.source);
}

// std/core/alloc/compiler-builtins are linked into native and WASM artifacts
// without appearing in Cargo.lock. Use the pinned toolchain's actual notices.
const sysroot = command("rustc", ["--print", "sysroot"]).trim();
const rustVersion = command("rustc", ["--version"]).trim();
const rustDocs = join(sysroot, "share/doc/rust");
addDocument(await readFile(join(rustDocs, "COPYRIGHT-library.html"), "utf8"),
  rustVersion, "Rust toolchain distribution: share/doc/rust/COPYRIGHT-library.html",
  "Conservative standard-library notice set, not a claim that every target-specific library component is linked.");
const library = join(sysroot, "lib/rustlib/src/rust/library");
for (const file of await legalFiles(library)) {
  addDocument(await readFile(join(library, file), "utf8"), rustVersion, `Rust source distribution: library/${file}`);
}
for (const item of await sourceHeaders(library)) {
  addDocument(item.text, rustVersion, `Rust source distribution: library/${item.name} (source notice)`);
}
for (const file of ["MIT.txt", "Apache-2.0.txt", "LLVM-exception.txt", "Unicode-3.0.txt", "NCSA.txt", "BSD-2-Clause.txt", "ISC.txt"]) {
  addDocument(await readFile(join(rustDocs, "licenses", file), "utf8"), rustVersion, `Rust distribution: licenses/${file}`);
}
const npm = JSON.parse(await readFile(join(root, "js/packages/polkavm-browser-runtime/package.json"), "utf8"));
for (const key of ["dependencies", "optionalDependencies", "peerDependencies", "bundledDependencies", "bundleDependencies"]) {
  if (Object.keys(npm[key] ?? {}).length) throw new Error(`Review browser npm ${key} before regenerating notices`);
}
const details = {
  schemaVersion: 1,
  cargoLockSha256: hash(lockText),
  rustToolchain: rustVersion,
  browser: { root: browserRoot.name, target: "wasm32-unknown-unknown", features: "default" },
  scope: "Normal/build paths only; dev edges excluded. All-targets-all-features is a conservative workspace superset, not a browser shipment claim. Build includes proc macros and their dependencies; generated output can contain their code.",
  packages: inventory,
};
let output = "THIRD-PARTY LICENSES AND NOTICES\n\n" +
  "Generated by scripts/generate-third-party-notices.mjs. See THIRD_PARTY_NOTICES.md.\n" +
  `Cargo.lock SHA-256: ${details.cargoLockSha256}\nRust: ${rustVersion}\n\n` +
  "SCOPE: browser-default/runtime = normal browser dependency path; browser-default/build = build/proc-macro path.\n" +
  "all-targets-all-features = conservative workspace normal/build superset, including optional native FFI/GPU and other targets.\n" +
  "An all-targets entry without browser-default is NOT asserted to ship in browser WASM.\n" +
  "Dev dependency edges are excluded. Dead-code elimination and feature unification are not a linkage audit.\n" +
  "Build tools may emit code into generated Rust/WASM; their notices are retained separately, not called browser runtime dependencies.\n" +
  "Authors are verbatim package metadata, not an assertion of copyright ownership.\n\n";
for (const pkg of inventory) {
  output += `${pkg.name} ${pkg.version}\nSource: ${pkg.source}\n` +
    `Registry checksum: ${pkg.checksum ?? "not applicable (git revision above)"}\n` +
    `Declared license: ${pkg.license ?? "see license file"}\nAuthors (metadata): ${pkg.authors.join("; ")}\n` +
    `Scopes: ${pkg.scopes.join(", ")}\nDocuments (SHA-256): ${pkg.documents.join(", ")}\n\n`;
}
for (const [digest, document] of [...documents].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) {
  output += `\n${"=".repeat(72)}\nDOCUMENT ${digest}\n`;
  for (const usage of [...document.uses.values()].sort((a, b) => JSON.stringify(a) < JSON.stringify(b) ? -1 : JSON.stringify(a) > JSON.stringify(b) ? 1 : 0)) {
    output += `For: ${usage.owner}\nSource: ${usage.source}\n${usage.note ? `Note: ${usage.note}\n` : ""}`;
  }
  output += `--- BEGIN VERBATIM UPSTREAM TEXT ---\n${document.text}${document.text.endsWith("\n") ? "" : "\n"}--- END VERBATIM UPSTREAM TEXT ---\n`;
}
for (const [file, content] of [
  ["licenses/dependency-inventory.json", JSON.stringify(details, null, 2) + "\n"],
  ["licenses/THIRD_PARTY_LICENSES.txt", output],
]) {
  if (content.includes(root) || content.includes(homedir())) throw new Error(`Local path leaked into ${file}`);
  if (check) {
    if (await readFile(join(root, file), "utf8") !== content) throw new Error(`${file} is stale; regenerate notices`);
  } else {
    await writeFile(join(root, file), content);
  }
}
console.log(`${check ? "Verified" : "Generated"} notices for ${inventory.length} Cargo packages and ${documents.size} distinct legal documents.`);
