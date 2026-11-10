import { test, expect, Viewer } from "./support/viewer.mjs";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(
    page.locator("#smoke-canvas[data-navigation-ready]"),
  ).toBeVisible();
  await page.evaluate(async () => {
    window.smokeInterop =
      await import("/_content/NifViewer.Blazor/nifviewer.interop.js");
    window.nifViewer = {
      navigation_status: () =>
        JSON.stringify(window.smokeInterop.navigationStatus()),
    };
  });
});

test("packaged component can be disposed and reattached while moving", async ({
  page,
}) => {
  const viewer = new Viewer(page);
  for (let cycle = 1; cycle <= 3; cycle++) {
    await test.step(`Attachment cycle ${cycle}`, async () => {
      await expect(
        page.locator("#smoke-canvas[data-navigation-ready]"),
      ).toBeVisible();
      await viewer.activate();
      await viewer.move();
      await page.keyboard.down("w");
      await page
        .getByRole("button", { name: "Attach / detach viewer" })
        .click();
      await page.keyboard.up("w");
      await expect(viewer.canvas).toHaveCount(0);
      await expect.poll(async () => (await viewer.status()).active).toBe(false);
      await page
        .getByRole("button", { name: "Attach / detach viewer" })
        .click();
    });
  }
});

test("packaged controls forward settings and host text input retains focus", async ({
  page,
}) => {
  const viewer = new Viewer(page);
  await viewer.activate();
  await page.locator("#host-field").fill("wasd");
  await expect.poll(async () => (await viewer.status()).active).toBe(false);
  const before = (await viewer.status()).eye;
  await page.getByRole("combobox").selectOption("Orbit");
  await expect
    .poll(async () => (await viewer.status()).settings.mode)
    .toBe("Orbit");
  expect((await viewer.status()).eye).toEqual(before);
});
