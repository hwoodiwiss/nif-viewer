import { test, expect } from "./support/viewer.mjs";

test("packaged interop attaches and forwards navigation settings", async ({
  viewer,
  page,
}) => {
  await page.evaluate(async () => {
    const interop = await import("/interop/nifviewer.interop.js");
    const canvas = document.querySelector("canvas");
    const old = await interop.attach(canvas.id);
    window.interop = interop;
    window.currentAttachment = await interop.attach(canvas.id);
    interop.detachNavigation(old);
    interop.setNavigationSettings({
      ...interop.navigationStatus().settings,
      speed: 7,
      automatic_speed: false,
    });
  });
  await expect.poll(async () => (await viewer.status()).settings.speed).toBe(7);
  await viewer.activate();
  await page.evaluate(() =>
    window.interop.detachNavigation(window.currentAttachment),
  );
  await expect.poll(async () => (await viewer.status()).active).toBe(false);
});
