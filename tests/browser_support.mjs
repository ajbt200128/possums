// Test-only process helpers. This still launches the buffered production router;
// the bounded Rust scenario channel is not wired to HTTP or /chat in packet 4.
import { spawn } from "node:child_process";
import { once } from "node:events";

export const FIXTURE_MODELS = ["fixture-model", "fixture-model-two"];
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

export async function startBufferedFixture() {
  const server = spawn("cargo", ["run", "--quiet", "--example", "browser_fixture"], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  const exit = once(server, "exit");
  const stop = async () => {
    if (server.exitCode === null && server.signalCode === null) server.kill("SIGTERM");
    await exit;
  };
  try {
    const origin = await new Promise((resolve, reject) => {
      let startup = Buffer.alloc(0);
      const timer = setTimeout(() => reject(new Error("fixture startup timed out")), 60_000);
      const finish = (error, value) => {
        clearTimeout(timer);
        server.stdout.off("data", receive);
        if (error) reject(error); else resolve(value);
      };
      const receive = (bytes) => {
        if (startup.length + bytes.length > MAX_ORIGIN_BYTES) {
          finish(new Error("fixture startup too large"));
          return;
        }
        startup = Buffer.concat([startup, bytes]);
        if (startup.includes(10)) {
          try { finish(null, fixtureOrigin(startup)); } catch (error) { finish(error); }
        }
      };
      server.stdout.on("data", receive);
      exit.then(() => finish(new Error("fixture exited")), () => finish(new Error("fixture failed")));
    });
    return { origin, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}
