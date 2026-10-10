import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { startStreamingFixture } from "./browser_support.mjs";

async function withExecutable(body, run) {
  const directory = await mkdtemp(join(tmpdir(), "possums-browser-launch-"));
  const executable = join(directory, "fixture");
  const previous = process.env.POSSUMS_BROWSER_FIXTURE;
  try {
    await writeFile(executable, `#!${process.execPath}\n${body}\n`, { mode: 0o700 });
    process.env.POSSUMS_BROWSER_FIXTURE = executable;
    await run();
  } finally {
    if (previous === undefined) delete process.env.POSSUMS_BROWSER_FIXTURE;
    else process.env.POSSUMS_BROWSER_FIXTURE = previous;
    await rm(directory, { recursive: true, force: true });
  }
}

test("prebuilt fixture launches without Cargo and preserves progressive IPC", async () => {
  await withExecutable(`
    if (process.argv.length !== 2 || process.env.POSSUMS_BROWSER_PROGRESSIVE !== "1") process.exit(1);
    console.log("http://127.0.0.1:1234");
    console.log("held");
    process.stdin.once("data", (bytes) => {
      if (bytes.toString() !== "R") process.exit(1);
      console.log("released");
    });
    setInterval(() => {}, 1000);
  `, async () => {
    const fixture = await startStreamingFixture({ progressive: true });
    try {
      assert.equal(fixture.origin, "http://127.0.0.1:1234");
      await fixture.waitFor("held");
      fixture.assertHeld();
      await fixture.release();
    } finally {
      await fixture.stop();
    }
  });
});

test("prebuilt fixture rejects hostile startup output with a closed error", async () => {
  await withExecutable(`
    console.log("hostile-startup-sentinel");
    setInterval(() => {}, 1000);
  `, async () => {
    await assert.rejects(startStreamingFixture(), { message: "invalid fixture origin" });
  });
});

test("configured fixture launch failure does not fall back to Cargo", async () => {
  await withExecutable("", async () => {
    await rm(process.env.POSSUMS_BROWSER_FIXTURE);
    await assert.rejects(startStreamingFixture(), { message: "fixture failed" });
  });
});
