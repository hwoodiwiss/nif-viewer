import { test, expect } from "./support/viewer.mjs";

test("stale disposal cannot deactivate a newer attachment", async ({
  viewer,
  page,
}) => {
  await page.evaluate(() => {
    const old = window.nifViewer.navigation_attach();
    window.currentAttachment = window.nifViewer.navigation_attach();
    window.nifViewer.navigation_detach(old);
  });
  await viewer.settings({ speed: 7 }); // Wait until queued attachment resets are processed.
  await viewer.activate();
  await viewer.move();
  await page.evaluate(() =>
    window.nifViewer.navigation_detach(window.currentAttachment),
  );
  await expect.poll(async () => (await viewer.status()).active).toBe(false);
});
