import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

await import("../src/pvm-computer.js");

const packageRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(packageRoot, "../../..");
const fixtureRoot = resolve(
  repositoryRoot,
  "rust/crates/pvm-runtime/tests/fixtures",
);

const {
  computerContext,
  ComputerProcess,
  ComputerSupervisor,
  ComputerTranslator,
  TTY_MODE_RAW,
} = globalThis.PvmComputer;

const MAX_GAS = 50_000_000;

const translator = await ComputerTranslator.create(
  await readFile(resolve(packageRoot, "dist/pvm-browser-runtime.wasm")),
);

async function fixture(name) {
  return translator.translate(
    await readFile(resolve(fixtureRoot, `${name}.polkavm`)),
  );
}

const coreContext = await fixture("computer-core-context");
const roundtrip = await fixture("computer-tty-fs-roundtrip");
const pipeDriver = await fixture("computer-pipe-driver");
const pipeFilter = await fixture("computer-pipe-filter");
const tcpRoundtrip = await fixture("computer-tcp-roundtrip");

const text = (bytes) => new TextDecoder().decode(bytes);

function runToExit(target, limit = 10_000) {
  for (let step = 0; step < limit; step++) {
    const status = target.run();
    if (status.kind === "exited") {
      return status.code;
    }
    assert.equal(status.kind, "yielded");
  }
  throw new Error("guest did not exit");
}

test("computer guest reads context and exits with status", () => {
  const context = computerContext(
    ["shell.polkavm", "--login"],
    [
      ["HOME", "/home"],
      ["TERM", "pvm-tty"],
    ],
  );
  const process = new ComputerProcess(coreContext, context, MAX_GAS);
  assert.deepEqual(process.run(), { kind: "exited", code: 23 });
  assert.deepEqual(process.run(), { kind: "exited", code: 23 });
});

test("computer guest roundtrips terminal and filesystem", () => {
  const process = new ComputerProcess(
    roundtrip,
    computerContext([], []),
    MAX_GAS,
  );
  process.setTerminalSize(100, 40);
  process.mountFile("/home/seed.txt", new TextEncoder().encode("seeded"));

  assert.equal(process.run().kind, "yielded");
  assert.equal(text(process.takeTerminalOutput()), "ready:seeded\r\n");
  assert.equal(process.terminalMode(), TTY_MODE_RAW);
  assert.deepEqual(process.takeModifiedFiles(), []);

  process.sendTerminalInput(new TextEncoder().encode("hello"));
  assert.equal(process.run().kind, "yielded");
  assert.equal(text(process.takeTerminalOutput()), "HELLO");

  process.sendTerminalInput(new TextEncoder().encode(" pvm"));
  assert.equal(process.run().kind, "yielded");
  assert.equal(text(process.takeTerminalOutput()), " PVM");

  process.sendTerminalInput(new TextEncoder().encode("q"));
  assert.deepEqual(process.run(), { kind: "exited", code: 7 });
  const modified = process.takeModifiedFiles();
  assert.equal(modified.length, 1);
  assert.equal(modified[0][0], "/home/echo.txt");
  assert.equal(text(modified[0][1]), "hello pvm");
});

test("guest streams bytes through a piped child and reaps it", () => {
  const supervisor = new ComputerSupervisor(
    pipeDriver,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("upper", pipeFilter);

  // The driver asserts every contract detail internally (unknown package,
  // bad pids, partial writes, EOF, double reap) and exits nonzero with a
  // distinct code on the first violation.
  assert.equal(runToExit(supervisor), 0);
  assert.equal(text(supervisor.takeTerminalOutput()), "HELLO, PIPES");
});

test("spawn without registration fails from the start", () => {
  const supervisor = new ComputerSupervisor(
    pipeDriver,
    computerContext([], []),
    MAX_GAS,
  );
  assert.equal(runToExit(supervisor), 13);
});

test("open spawn suspends for the embedder and resumes", () => {
  const supervisor = new ComputerSupervisor(
    pipeDriver,
    computerContext([], []),
    MAX_GAS,
    null,
    { packageResolution: true },
  );
  const requested = [];
  for (let step = 0; step < 10_000; step++) {
    const status = supervisor.run();
    if (status.kind === "exited") {
      assert.equal(status.code, 0);
      assert.equal(text(supervisor.takeTerminalOutput()), "HELLO, PIPES");
      // The driver probes one unknown package (rejected -> NOT_FOUND, the
      // same observable as the default path) and pipes through "upper"
      // (provided by the embedder without prior registration).
      assert.ok(requested.includes("upper"));
      assert.ok(requested.some((name) => name !== "upper"));
      return;
    }
    if (status.kind === "package") {
      // Suspension is idempotent until the embedder acts.
      assert.deepEqual(supervisor.run(), status);
      requested.push(status.package);
      if (status.package === "upper") {
        supervisor.providePackage(pipeFilter);
      } else {
        supervisor.rejectPackage();
      }
      continue;
    }
    assert.equal(status.kind, "yielded");
  }
  throw new Error("guest did not exit");
});

test("supervisor terminates the root as interrupted", () => {
  const supervisor = new ComputerSupervisor(
    roundtrip,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.setTerminalSize(100, 40);
  supervisor.mountFile(
    "/home/seed.txt",
    new TextEncoder().encode("seeded"),
  );

  assert.equal(supervisor.run().kind, "yielded");
  supervisor.sendTerminalInput(new TextEncoder().encode("hello"));
  assert.equal(supervisor.run().kind, "yielded");

  assert.deepEqual(supervisor.terminateForeground(), {
    kind: "exited",
    code: 130,
  });
  assert.deepEqual(supervisor.takeModifiedFiles(), []);
  // Termination is recorded: the computer stays exited on later runs.
  assert.deepEqual(supervisor.run(), { kind: "exited", code: 130 });
});

test("network capability reports denied on the web host", () => {
  const process = new ComputerProcess(
    tcpRoundtrip,
    computerContext([], [["NET_TARGET", "127.0.0.1:1"]]),
    MAX_GAS,
  );
  // The tcp fixture maps a DENIED connect to its distinct exit code 21.
  assert.equal(runToExit(process), 21);
});
