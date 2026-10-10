// Test-only IPC for the production streaming /chat router. No HTTP control route.
import { spawn } from "node:child_process";

export const FIXTURE_MODELS = ["fixture-model", "fixture-model-two"];
export const FIXTURE_TEXT = "<script>fetch('https://browser-canary.invalid')</script> ![pixel](https://browser-canary.invalid/pixel) **safe response**";
export const FIXTURE_PARTIAL = FIXTURE_TEXT.slice(0, -" **safe response**".length);
const MAX_ORIGIN_BYTES = 128;

export function fixtureOrigin(bytes) {
  if (bytes.length > MAX_ORIGIN_BYTES) throw new Error("fixture startup too large");
  const origin = bytes.toString().trim();
  if (!/^http:\/\/127\.0\.0\.1:[1-9][0-9]{0,4}$/.test(origin)) {
    throw new Error("invalid fixture origin"); // Never echo arbitrary child output.
  }
  if (Number(origin.split(":").at(-1)) > 65535) throw new Error("invalid fixture port");
  return origin;
}

export async function startStreamingFixture({ progressive = false } = {}) {
  // CI runs the cached Nix fixture directly; local development retains cargo run.
  // A process group ensures cargo AND the example die on failure, not just cargo.
  const executable = process.env.POSSUMS_BROWSER_FIXTURE;
  const server = spawn(executable || "cargo", executable ? [] : ["run", "--quiet", "--example", "browser_fixture"], {
    detached: true,
    env: { ...process.env, POSSUMS_BROWSER_PROGRESSIVE: progressive ? "1" : "0" },
    stdio: ["pipe", "pipe", "inherit"],
  });
  const phases = ["ready", "held", "released", "second-accepted", "reset-accepted"];
  const waiters = new Set();
  let phase = -1;
  let origin;
  let pending = Buffer.alloc(0);
  let failure;
  let stopping = false;
  let releaseRequested = false;
  let killTimer;
  const killGroup = (signal) => {
    if (!server.pid) return;
    try { process.kill(-server.pid, signal); } catch (error) {
      if (error.code !== "ESRCH") throw new Error("fixture cleanup failed");
    }
  };
  const terminate = () => {
    if (killTimer) return;
    killGroup("SIGTERM");
    killTimer ??= setTimeout(() => killGroup("SIGKILL"), 2_000);
  };
  const notify = () => { for (const waiter of waiters) waiter(); };
  const fail = (message) => {
    failure ??= new Error(message);
    notify();
    terminate();
  };
  const closed = new Promise((resolve) => {
    server.once("error", () => fail("fixture failed"));
    server.once("close", () => {
      if (!stopping) fail("fixture exited");
      clearTimeout(killTimer);
      resolve();
    });
  });
  server.stdin.on("error", () => fail("fixture control failed"));
  server.stdout.on("data", (bytes) => {
    if (failure || stopping) return;
    if (pending.length + bytes.length > 1024) return fail("fixture output too large");
    pending = Buffer.concat([pending, bytes]);
    let newline;
    while ((newline = pending.indexOf(10)) !== -1) {
      const line = pending.subarray(0, newline);
      pending = pending.subarray(newline + 1);
      if (phase === -1) {
        try { origin = fixtureOrigin(line); } catch { return fail("invalid fixture origin"); }
      } else if (!progressive || line.toString() !== phases[phase + 1]
        || (phase === 1 && !releaseRequested)) {
        return fail("unexpected fixture phase");
      }
      phase += 1;
      notify();
    }
    if (pending.length > MAX_ORIGIN_BYTES) fail("fixture output too large");
  });
  const waitFor = (expected, timeout = 10_000) => new Promise((resolve, reject) => {
    const target = phases.indexOf(expected);
    if (target === -1) return reject(new Error("invalid fixture phase"));
    const timer = setTimeout(() => fail("fixture phase timed out"), timeout);
    const check = () => {
      if (!failure && phase < target) return;
      clearTimeout(timer);
      waiters.delete(check);
      if (failure) reject(failure); else resolve();
    };
    waiters.add(check);
    check();
  });
  const assertHeld = () => {
    if (failure) throw failure;
    if (phase !== 1 || releaseRequested) throw new Error("fixture terminal is not held");
  };
  const stop = async () => {
    stopping = true;
    terminate();
    await closed;
    clearTimeout(killTimer);
  };
  try {
    await waitFor("ready", 60_000);
    return {
      origin, stop, waitFor, assertHeld,
      release: async () => {
        assertHeld();
        releaseRequested = true;
        server.stdin.end("R");
        await waitFor("released");
      },
    };
  } catch (error) {
    await stop();
    throw error;
  }
}
