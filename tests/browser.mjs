import assert from "node:assert/strict";
import { chromium } from "playwright";
import {
  fixtureOrigin, startStreamingFixture, FIXTURE_MODELS, FIXTURE_TEXT, FIXTURE_PARTIAL,
} from "./browser_support.mjs";

let stage = "startup";
async function acceptance() {
  // Direct helper boundary checks; no prompt/credential data in process diagnostics.
  assert.equal(fixtureOrigin(Buffer.from("http://127.0.0.1:1234\n")), "http://127.0.0.1:1234");
  for (const invalid of ["https://remote.invalid", "http://127.0.0.1:65536", "x".repeat(129)]) {
    assert.throws(() => fixtureOrigin(Buffer.from(invalid)));
  }
  const fixture = await startStreamingFixture({ progressive: true });
  const { origin, stop } = fixture;
  try {

    const browser = await chromium.launch({ headless: true });
    try {
      const context = await browser.newContext({ javaScriptEnabled: false });
      context.setDefaultTimeout(10_000);
      context.setDefaultNavigationTimeout(10_000);
      let remoteRequested = false;
      // Prevent egress even on regression; only retain a boolean, never URLs.
      await context.route("**/*", async (route) => {
        if (new URL(route.request().url()).origin !== origin) {
          remoteRequested = true;
          await route.abort();
        } else {
          await route.continue();
        }
      });
      context.on("request", (request) => {
        if (new URL(request.url()).origin !== origin) remoteRequested = true;
      });
      const page = await context.newPage();
      const noActiveOutput = async () => {
        assert.equal(await page.locator("script, img, iframe, object, embed, audio, video, source, svg, math").count(), 0);
        assert.equal(remoteRequested, false);
      };
      stage = "authentication";
      await page.goto(origin);
      if ((await page.locator("script").count()) !== 0) throw new Error("script element rendered");

      const credential = Buffer.alloc(32, 7).toString("base64url");
      await page.locator('input[name="credential"]').fill(credential);
      await Promise.all([
        page.waitForURL(`${origin}/`),
        page.getByRole("button", { name: "Log in" }).click(),
      ]);
      assert.deepEqual(await page.locator('select[name="model"] option').evaluateAll(
        (options) => options.map((option) => option.value),
      ), FIXTURE_MODELS);
      await page.locator('select[name="model"]').selectOption(FIXTURE_MODELS[0]);

      stage = "manual recovery";
      await page.getByRole("link", { name: "Recovery credential" }).click();
      const [download] = await Promise.all([
        page.waitForEvent("download"),
        page.getByRole("link", { name: "Download recovery credential" }).click(),
      ]);
      if (download.suggestedFilename() !== "possums-recovery.txt") {
        throw new Error("unexpected recovery filename");
      }
      await page.getByRole("link", { name: "Return" }).click();

      stage = "progressive commit navigation";
      await page.locator('textarea[name="prompt"]').fill("first turn");
      await Promise.all([
        page.waitForURL(`${origin}/chat`, { waitUntil: "commit" }),
        page.getByRole("button", { name: "Send" }).click({ noWaitAfter: true }),
        fixture.waitFor("held"),
      ]);
      stage = "held partial DOM";
      fixture.assertHeld();
      const assistant = page.locator('pre[aria-label="Assistant"]');
      await assistant.filter({ hasText: FIXTURE_PARTIAL }).waitFor({ state: "visible" });
      // Driver-side inspection only: no page script or DOM mutation. This must be
      // actual parsed, visible text while final usage/EOF is still unreleased.
      assert.equal(await assistant.textContent(), FIXTURE_PARTIAL);
      assert.equal(await assistant.locator("*").count(), 0);
      assert.equal(await page.evaluate(() => document.readyState), "loading");
      await page.getByText("first turn", { exact: true }).waitFor();
      assert.equal(await page.getByRole("button", { name: "Send", exact: true }).count(), 0);
      assert.equal(await page.locator('textarea[name="prompt"], input[name="history_manifest"]').count(), 0);
      await page.getByRole("button", { name: "New chat", exact: true }).waitFor();
      await page.getByRole("button", { name: "Log out", exact: true }).waitFor();
      await page.getByRole("link", { name: "Recovery credential", exact: true }).waitFor();
      await noActiveOutput();
      fixture.assertHeld(); // The assertions themselves must not release the gate.

      stage = "released continuation";
      await fixture.release();
      await page.getByRole("button", { name: "Send", exact: true }).waitFor();
      await page.waitForLoadState("load");
      assert.equal(await assistant.textContent(), FIXTURE_TEXT);
      await noActiveOutput();
      assert.equal(await page.locator('select[name="model"]').count(), 0);
      await page.getByText(`Selected model: ${FIXTURE_MODELS[0]}`, { exact: true }).waitFor();
      if ((await page.locator('input[name="model"]').inputValue()) !== "fixture-model") {
        throw new Error("locked model was not preserved");
      }

      stage = "exact upstream continuation";
      await page.locator('textarea[name="prompt"]').fill("second turn");
      await page.getByRole("button", { name: "Send" }).click();
      await fixture.waitFor("second-accepted");
      await page.getByText("second turn", { exact: true }).waitFor();
      await page.getByRole("button", { name: "Send", exact: true }).waitFor();
      assert.equal(await assistant.textContent(), FIXTURE_TEXT);
      await noActiveOutput();

      stage = "new chat reset";

      const resetForm = page.locator('form[action="/chat/new"]');
      assert.equal(await resetForm.count(), 1);
      assert.deepEqual(await resetForm.locator("input").evaluateAll(
        (inputs) => inputs.map((input) => input.name),
      ), ["csrf"]);
      assert.equal(await resetForm.locator('input[name="csrf"]').inputValue(),
        await page.locator('form[action="/chat"] input[name="csrf"]').inputValue());
      await resetForm.getByRole("button", { name: "New chat" }).click();
      await page.waitForURL(`${origin}/chat/new`);
      assert.equal(await page.locator("pre").count(), 0);
      assert.equal(await page.locator('form[action="/chat"] input[name="h000000"]').inputValue(), "W10");
      assert.equal(await page.locator('form[action="/chat"] input[name="history_manifest"]').inputValue(), "1.000001.00000002");
      assert.equal(await page.locator('select[name="model"]').isEnabled(), true);
      assert.equal(await page.locator('form[action="/chat"] input[name="model"]').count(), 0);
      await page.locator('select[name="model"]').selectOption(FIXTURE_MODELS[1]);
      await page.locator('textarea[name="prompt"]').fill("after reset");
      await page.getByRole("button", { name: "Send" }).click();
      await fixture.waitFor("reset-accepted");
      await page.getByText("after reset", { exact: true }).waitFor();
      await page.getByRole("button", { name: "Send", exact: true }).waitFor();
      assert.equal(await assistant.textContent(), FIXTURE_TEXT);
      await noActiveOutput();
      assert.equal(await page.locator("pre").count(), 2);
      assert.equal(await page.getByText("first turn").count(), 0);
      assert.equal(await page.getByText("second turn").count(), 0);
      assert.equal(await page.locator('select[name="model"]').count(), 0);
      await page.getByText(`Selected model: ${FIXTURE_MODELS[1]}`, { exact: true }).waitFor();
      assert.equal(await page.locator('input[name="model"]').inputValue(), FIXTURE_MODELS[1]);

      stage = "logout and fresh login";
      await page.getByRole("button", { name: "Log out" }).click();
      await page.waitForURL(`${origin}/`);
      await page.locator('input[name="credential"]').fill(credential);
      await page.getByRole("button", { name: "Log in" }).click();
      assert.equal(await page.locator('select[name="model"]').isEnabled(), true);
      assert.equal(await page.locator("article").count(), 0);
      assert.deepEqual(await page.locator('select[name="model"] option').evaluateAll(
        (options) => options.map((option) => option.value),
      ), FIXTURE_MODELS);
      await noActiveOutput();
      await context.close();
    } finally {
      await browser.close();
    }
  } finally {
    await stop();
  }
}

try {
  await acceptance();
} catch {
  // Playwright errors can include page URLs, selector text and input values.
  // Report only a fixed phase, never the original error/cause or page content.
  console.error(`browser acceptance failed: ${stage}`);
  process.exitCode = 1;
}
