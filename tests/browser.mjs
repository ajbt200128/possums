import assert from "node:assert/strict";
import { chromium } from "playwright";
import { fixtureOrigin, startBufferedFixture, FIXTURE_MODELS } from "./browser_support.mjs";

// Direct helper boundary checks; no prompt/credential data in process diagnostics.
assert.equal(fixtureOrigin(Buffer.from("http://127.0.0.1:1234\n")), "http://127.0.0.1:1234");
for (const invalid of ["https://remote.invalid", "http://127.0.0.1:65536", "x".repeat(129)]) {
  assert.throws(() => fixtureOrigin(Buffer.from(invalid)));
}
const { origin, stop } = await startBufferedFixture();
try {

  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({ javaScriptEnabled: false });
    const remoteRequests = [];
    context.on("request", (request) => {
      if (!request.url().startsWith(origin)) remoteRequests.push(request.url());
    });
    const page = await context.newPage();
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

    const downloadPromise = page.waitForEvent("download");
    await page.getByRole("link", { name: "Recovery credential" }).click();
    await page.getByRole("link", { name: "Download recovery credential" }).click();
    const download = await downloadPromise;
    if (download.suggestedFilename() !== "possums-recovery.txt") {
      throw new Error("unexpected recovery filename");
    }
    await page.getByRole("link", { name: "Return" }).click();

    await page.locator('textarea[name="prompt"]').fill("first turn");
    await page.getByRole("button", { name: "Send" }).click();
    await page.getByText("first turn").waitFor();
    if ((await page.locator("script, img, iframe").count()) !== 0) {
      throw new Error("hostile output rendered an active or remote element");
    }
    await page.getByText("safe response", { exact: false }).waitFor();
    assert.equal(await page.locator('select[name="model"]').count(), 0);
    await page.getByText(`Selected model: ${FIXTURE_MODELS[0]}`, { exact: true }).waitFor();
    if ((await page.locator('input[name="model"]').inputValue()) !== "fixture-model") {
      throw new Error("locked model was not preserved");
    }

    await page.locator('textarea[name="prompt"]').fill("second turn");
    await page.getByRole("button", { name: "Send" }).click();
    await page.getByText("second turn").waitFor();

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
    await page.getByText("after reset").waitFor();
    assert.equal(await page.locator("pre").count(), 2);
    assert.equal(await page.getByText("first turn").count(), 0);
    assert.equal(await page.getByText("second turn").count(), 0);
    assert.equal(await page.locator('select[name="model"]').count(), 0);
    await page.getByText(`Selected model: ${FIXTURE_MODELS[1]}`, { exact: true }).waitFor();
    assert.equal(await page.locator('input[name="model"]').inputValue(), FIXTURE_MODELS[1]);

    await page.getByRole("button", { name: "Log out" }).click();
    await page.waitForURL(`${origin}/`);
    await page.locator('input[name="credential"]').fill(credential);
    await page.getByRole("button", { name: "Log in" }).click();
    assert.equal(await page.locator('select[name="model"]').isEnabled(), true);
    assert.equal(await page.locator("article").count(), 0);
    assert.deepEqual(await page.locator('select[name="model"] option').evaluateAll(
      (options) => options.map((option) => option.value),
    ), FIXTURE_MODELS);
    if (remoteRequests.length !== 0) {
      throw new Error(`remote browser requests observed: ${remoteRequests.join(", ")}`);
    }
    await context.close();
  } finally {
    await browser.close();
  }
} finally {
  await stop();
}
