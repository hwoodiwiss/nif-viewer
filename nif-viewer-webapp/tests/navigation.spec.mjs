import { test, expect, displacement } from "./support/viewer.mjs";

test("keyboard movement and Escape release keep the viewer usable", async ({
  viewer,
}) => {
  await viewer.activate();
  await viewer.move();
  await viewer.release();
  await viewer.activate();
  await viewer.move("d");
});

test("keyboard look and Fly/Orbit switching preserve the camera position", async ({
  viewer,
  page,
}) => {
  await viewer.activate();
  const before = await viewer.status();
  await page.keyboard.down("ArrowRight");
  try {
    await expect
      .poll(async () =>
        displacement((await viewer.status()).target, before.target),
      )
      .toBeGreaterThan(0.05);
  } finally {
    await page.keyboard.up("ArrowRight");
  }
  await viewer.release();
  const position = (await viewer.status()).eye;
  await viewer.activate();
  await page.keyboard.press("o");
  await expect
    .poll(async () => (await viewer.status()).settings.mode)
    .toBe("Orbit");
  expect(displacement((await viewer.status()).eye, position)).toBeLessThan(
    0.001,
  );
  await page.keyboard.press("o");
  await expect
    .poll(async () => (await viewer.status()).settings.mode)
    .toBe("Fly");
});

test("right-button drag changes look direction without pointer lock", async ({
  viewer,
  page,
}) => {
  await viewer.activate();
  const before = (await viewer.status()).target;
  const box = await viewer.canvas.boundingBox();
  await page.mouse.move(box.x + 100, box.y + 100);
  await page.mouse.down({ button: "right" });
  try {
    await page.mouse.move(box.x + 200, box.y + 130, { steps: 8 });
  } finally {
    await page.mouse.up({ button: "right" });
  }
  await expect
    .poll(async () => displacement((await viewer.status()).target, before))
    .toBeGreaterThan(0.05);
  expect((await viewer.status()).captured).toBe(false);
});

test("blur clears a held key and typing in a host field does not navigate", async ({
  viewer,
  page,
}) => {
  const field = page.locator("#host-field");
  await page.evaluate(() => {
    if (!document.getElementById("host-field")) {
      const input = document.createElement("input");
      input.id = "host-field";
      document.body.prepend(input);
    }
  });
  await viewer.activate();
  await page.keyboard.down("w");
  await field.focus();
  await page.keyboard.up("w");
  await expect.poll(async () => (await viewer.status()).active).toBe(false);
  const stopped = (await viewer.status()).eye;
  await field.pressSequentially("wasd");
  await viewer.activate();
  // Waiting for a settings round trip also establishes a subsequent WASM update.
  await viewer.settings({ speed: 9 });
  expect((await viewer.status()).eye).toEqual(stopped);
});

test("Tab leaves the canvas and deactivates navigation", async ({
  viewer,
  page,
}) => {
  await viewer.activate();
  await page.keyboard.press("Tab");
  await expect(viewer.canvas).not.toBeFocused();
  await expect.poll(async () => (await viewer.status()).active).toBe(false);
});

test("physical key bindings can be replaced", async ({ viewer, page }) => {
  await page.evaluate(() =>
    window.nifViewer.set_navigation_bindings(
      JSON.stringify({
        Enter: "Activate",
        Escape: "Release",
        KeyI: "Forward",
        KeyK: "Backward",
      }),
    ),
  );
  await viewer.settings({ speed: 10 });
  await viewer.activate();
  await viewer.move("i");
  await viewer.release();
});
