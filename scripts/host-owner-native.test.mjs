import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { join, isAbsolute } from "node:path";
import { launchOwnedProcess } from "../bin/host-owner.js";
import { startApplication } from "../bin/host-app.js";

const owner = process.env.LENSO_NATIVE_OWNER;
assert.ok(owner && isAbsolute(owner), "set LENSO_NATIVE_OWNER to the built native helper; no fake fallback");
const runtime = process.env.LENSO_APPLICATION_RUNTIME;

test("TypeScript ownership handshake, duplicate-root denial, stop, and restart use the real helper", async () => {
  const directory = mkdtempSync(join(tmpdir(), "lenso-native-owner-"));
  const root = join(directory, "app"); mkdirSync(root);
  const options = { owner, distribution: "native-test-v1", root, registry: join(directory, "owners"),
    executable: "/bin/sh", arguments: ["-c", "trap '' TERM; sleep 60 & wait"],
    startupMs: 2000, stopMs: 50, confirmationMs: 2000 };
  let app;
  try {
    app = await launchOwnedProcess(options);
    assert.ok(app.pid > 0);
    await assert.rejects(launchOwnedProcess(options), /startup failed/);
    const first = app.stop();
    assert.equal(app.stop(), first);
    const outcome = await first;
    assert.equal(outcome.termination, "confirmed");
    assert.equal(outcome.forced, true, "ownership cleanup does not claim graceful business shutdown");
    assert.deepEqual(await app.closed, outcome);
    app = await launchOwnedProcess(options);
    assert.equal((await app.stop()).termination, "confirmed");
  } finally {
    if (app) await app.stop();
    rmSync(directory, { recursive: true, force: true });
  }
});

test("killing the JavaScript launcher leaves cleanup with the native owner", async () => {
  const directory = mkdtempSync(join(tmpdir(), "lenso-killed-launcher-"));
  const root = join(directory, "app"); mkdirSync(root);
  const registry = join(directory, "owners");
  const output = join(directory, "owned.json");
  const options = { owner, distribution: "native-test-v1", root, registry,
    executable: "/bin/sh", arguments: ["-c", "trap '' TERM; sleep 60 & wait"],
    startupMs: 2000, stopMs: 50, confirmationMs: 2000 };
  const launcher = spawn(process.execPath, [fileURLToPath(new URL("./fixtures/owner-launcher.mjs", import.meta.url))], {
    stdio: "ignore", env: { ...process.env, LENSO_OWNER_TEST_OPTIONS: JSON.stringify(options), LENSO_OWNER_TEST_OUTPUT: output },
  });
  const exited = once(launcher, "exit");
  const waitFor = async predicate => {
    const deadline = performance.now() + 5000;
    while (!predicate()) {
      assert.ok(performance.now() < deadline, "bounded native cleanup");
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  };
  try {
    await waitFor(() => existsSync(output));
    const { pid } = JSON.parse(readFileSync(output, "utf8"));
    assert.ok(launcher.kill("SIGKILL"));
    await exited;
    await waitFor(() => readdirSync(registry).every(name => readFileSync(join(registry, name), "utf8") === "settled\n"));
    assert.throws(() => process.kill(pid, 0), error => error.code === "ESRCH");
    let restarted;
    const restartDeadline = performance.now() + 5000;
    while (!restarted) {
      try {
        restarted = await launchOwnedProcess(options);
      } catch {
        assert.ok(performance.now() < restartDeadline, "native owner releases the execution lock");
        await new Promise(resolve => setTimeout(resolve, 10));
      }
    }
    assert.equal((await restarted.stop()).termination, "confirmed");
  } finally {
    if (launcher.exitCode === null && launcher.signalCode === null) launcher.kill("SIGKILL");
    await exited;
    rmSync(directory, { recursive: true, force: true });
  }
});

test("TypeScript start, readiness, inspection, durable stop, and ownership confirmation use the real Rust bridge", async () => {
  assert.ok(runtime && isAbsolute(runtime), "set LENSO_APPLICATION_RUNTIME to the built Rust control fixture");
  const directory = mkdtempSync(join(tmpdir(), "lenso-control-bridge-"));
  const root = join(directory, "app"); mkdirSync(root);
  let app;
  try {
    app = await startApplication({ owner, distribution: "control-test-v1", root, registry: join(directory, "owners"),
      executable: runtime, arguments: ["control-test-v1"], startupMs: 2000, stopMs: 2000, confirmationMs: 2000 });
    const snapshot = await app.inspect();
    assert.ok(Number.isSafeInteger(snapshot.revision)); assert.match(snapshot.generation, /^sha256:/); assert.deepEqual(snapshot.instances, []); assert.deepEqual(snapshot.diagnostics, []);
    const stopped = app.stop();
    assert.equal(await app.stop(), await stopped);
    const outcome = await stopped;
    assert.equal(outcome.shutdown, "suspended");
    assert.equal(outcome.ownership.termination, "confirmed", JSON.stringify(outcome));
    assert.deepEqual(await app.closed, outcome);
  } finally {
    if (app) await app.stop();
    rmSync(directory, { recursive: true, force: true });
  }
});
