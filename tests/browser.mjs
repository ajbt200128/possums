import { spawn } from "node:child_process";
import { once } from "node:events";
import { chromium } from "playwright";

const server = spawn("cargo", ["run", "--quiet", "--example", "browser_fixture"], {
  stdio: ["ignore", "pipe", "inherit"],
});

try {
  const [chunk] = await once(server.stdout, "data");
  const origin = chunk.toString().trim();
  if (!origin.startsWith("http://127.0.0.1:")) {
    throw new Error(`fixture did not report an origin: ${origin}`);
  }

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
    await page.locator('select[name="model"]').selectOption("fixture-model");

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
    await page.getByText("Confirm delivery to finalize the charge.").waitFor();
    if ((await page.locator("script, img, iframe").count()) !== 0) {
      throw new Error("hostile output rendered an active or remote element");
    }
    await page.getByRole("button", { name: "Confirm response delivery" }).click();
    await page.getByText("Delivery confirmed.").waitFor();
    await page.getByText("first turn").waitFor();
    await page.getByText("safe response").waitFor();

    await page.locator('textarea[name="prompt"]').fill("second turn");
    await page.getByRole("button", { name: "Send" }).click();
    await page.getByText("second turn").waitFor();
    if (remoteRequests.length !== 0) {
      throw new Error(`remote browser requests observed: ${remoteRequests.join(", ")}`);
    }
    await context.close();
  } finally {
    await browser.close();
  }
} finally {
  server.kill("SIGTERM");
  await once(server, "exit").catch(() => {});
}
