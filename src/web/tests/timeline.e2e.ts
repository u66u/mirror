import { expect, type Page, test } from "@playwright/test";

const previewBmp = createBmpFixture(64, 48);

test("timeline pages, virtualizes, and opens preview", async ({ page }, testInfo) => {
  await page.route("**/auth/login", async (route) => {
    await route.fulfill({ status: 204 });
  });
  await page.route("**/assets/*/derivatives/*", async (route) => {
    await route.fulfill({ body: previewBmp, contentType: "image/bmp", status: 200 });
  });
  await page.route("**/assets?**", async (route) => {
    const url = new URL(route.request().url());
    const cursor = url.searchParams.get("cursor");
    const start = cursor === null ? 0 : 120;
    const count = cursor === null ? 120 : 60;
    await route.fulfill({
      contentType: "application/json",
      status: 200,
      body: JSON.stringify({
        items: Array.from({ length: count }, (_, offset) =>
          assetDto(start + offset)
        ),
        next_cursor: cursor === null ? "page-2" : null
      })
    });
  });

  await page.goto("/");
  await page.getByRole("button", { name: "Login" }).first().click();
  await page.getByLabel("Password").fill("correct horse battery staple");
  await page.getByRole("button", { name: "Login" }).last().click();

  await expect(page.getByText("120 loaded")).toBeVisible();
  const renderedAssetCount = await page.locator('button[aria-label^="Open "]').count();
  expect(renderedAssetCount).toBeGreaterThan(0);
  expect(renderedAssetCount).toBeLessThan(120);
  await waitForImages(page);
  await page.screenshot({
    fullPage: true,
    path: testInfo.outputPath("timeline-desktop.png")
  });

  await page.getByRole("button", { name: "Open photo-000.jpg" }).click();
  await expect(page.getByRole("dialog", { name: "photo-000.jpg" })).toBeVisible();
  await expect(page.getByRole("img", { name: "photo-000.jpg preview" })).toBeVisible();
  await waitForImages(page);
  await page.screenshot({
    fullPage: true,
    path: testInfo.outputPath("preview-desktop.png")
  });
  await page.getByRole("button", { name: "Close preview" }).click();

  await page.getByRole("button", { name: "Load more" }).click();
  await expect(page.getByText("180 loaded")).toBeVisible();
  await expect(page.getByRole("button", { name: "Load more" })).toHaveCount(0);

  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator(".asset-scroll").evaluate((element) => {
    element.scrollTop = 0;
  });
  await waitForImages(page);
  await page.screenshot({
    fullPage: true,
    path: testInfo.outputPath("timeline-mobile.png")
  });
});

function assetDto(index: number) {
  return {
    asset_id: `019b0000-0000-7000-8000-${String(index).padStart(12, "0")}`,
    created_at: new Date(Date.UTC(2026, 5, 15, 12, 0, -index)).toISOString(),
    favorite_at: null,
    original_blake3: "a".repeat(64),
    media_type: index % 9 === 0 ? "video/mp4" : "image/jpeg",
    size_bytes: 2_400_000 + index,
    original_filename: `photo-${String(index).padStart(3, "0")}.jpg`,
    thumbnail: { format: "webp", width: 512, height: 384 },
    preview: { format: "webp", width: 1600, height: 1200 }
  };
}

function createBmpFixture(width: number, height: number): Buffer {
  const rowBytes = Math.ceil((width * 3) / 4) * 4;
  const pixelBytes = rowBytes * height;
  const output = Buffer.alloc(54 + pixelBytes);
  output.write("BM", 0, "ascii");
  output.writeUInt32LE(output.length, 2);
  output.writeUInt32LE(54, 10);
  output.writeUInt32LE(40, 14);
  output.writeInt32LE(width, 18);
  output.writeInt32LE(height, 22);
  output.writeUInt16LE(1, 26);
  output.writeUInt16LE(24, 28);
  output.writeUInt32LE(pixelBytes, 34);

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const teal = (Math.floor(x / 8) + Math.floor(y / 8)) % 2 === 0;
      const offset = 54 + y * rowBytes + x * 3;
      output[offset] = teal ? 106 : 65;
      output[offset + 1] = teal ? 107 : 72;
      output[offset + 2] = teal ? 27 : 190;
    }
  }
  return output;
}

async function waitForImages(page: Page): Promise<void> {
  await page.locator("img").evaluateAll(async (images) => {
    await Promise.all(
      images.map(async (image) => {
        const htmlImage = image as HTMLImageElement;
        htmlImage.loading = "eager";
        await htmlImage.decode().catch(() => undefined);
      })
    );
  });
}
