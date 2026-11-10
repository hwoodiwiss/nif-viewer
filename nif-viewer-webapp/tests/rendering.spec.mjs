import {
  test,
  expect,
  foregroundFraction,
  changedPixelFraction,
  displacement,
} from "./support/viewer.mjs";
import { navigationNif } from "./fixtures/navigation-nif.mjs";

test("generated landmarks render and the displayed image changes after turning", async ({
  viewer,
  page,
}, testInfo) => {
  const initial = (await viewer.status()).eye;
  await page.evaluate((bytes) => {
    window.nifViewer.set_light(1, 1, 1, -0.4, -0.8, -0.45, 8, 0.25, false);
    window.nifViewer.load_nif(
      "generated-navigation.nif",
      new Uint8Array(bytes),
      {},
    );
  }, navigationNif());
  await expect
    .poll(async () => (await viewer.status()).eye)
    .not.toEqual(initial);
  let before;
  await expect
    .poll(
      async () => {
        before = await viewer.screenshot();
        return foregroundFraction(before);
      },
      {
        message:
          "Generated landmarks cover at least 1% of the composited canvas",
      },
    )
    .toBeGreaterThan(0.01);
  await testInfo.attach("landmarks-before.png", {
    body: before,
    contentType: "image/png",
  });
  await viewer.activate();
  const directionBefore = (await viewer.status()).target;
  const box = await viewer.canvas.boundingBox();
  await page.mouse.move(box.x + 100, box.y + 100);
  await page.mouse.down({ button: "right" });
  await page.mouse.move(box.x + 280, box.y + 100, { steps: 10 });
  await page.mouse.up({ button: "right" });
  await expect
    .poll(async () =>
      displacement((await viewer.status()).target, directionBefore),
    )
    .toBeGreaterThan(0.05);
  await viewer.release();
  let after;
  await expect
    .poll(
      async () => {
        after = await viewer.screenshot();
        return changedPixelFraction(before, after);
      },
      { message: "Turning changes at least 1% of displayed pixels" },
    )
    .toBeGreaterThan(0.01);
  await testInfo.attach("landmarks-after.png", {
    body: after,
    contentType: "image/png",
  });
});
