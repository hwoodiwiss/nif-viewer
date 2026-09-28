import { test, expect, displacement } from "./support/viewer.mjs";
import { installGamepads, setGamepads, xbox } from "./support/gamepad.mjs";

test.beforeEach(async ({ page }) => {
  await installGamepads(page);
});

test("a standard controller activates, moves and stops on disconnect", async ({
  viewer,
  page,
}) => {
  await viewer.canvas.focus();
  const pad = xbox();
  await setGamepads(page, [null, pad]); // Neutral state arms controller selection.
  pad.buttons[0] = 1;
  await setGamepads(page, [null, pad]);
  await expect.poll(async () => (await viewer.status()).active).toBe(true);
  expect((await viewer.status()).device).toBe("Test Xbox");
  const before = (await viewer.status()).eye;
  pad.axes[1] = -1;
  await setGamepads(page, [null, pad]);
  await expect
    .poll(async () => displacement((await viewer.status()).eye, before))
    .toBeGreaterThan(0.2);
  await setGamepads(page, []);
  await expect.poll(async () => (await viewer.status()).active).toBe(false);
  const stopped = (await viewer.status()).eye;
  await viewer.settings({ speed: 8 });
  expect((await viewer.status()).eye).toEqual(stopped);
});

test("a held button on connection does not activate until released and pressed again", async ({
  viewer,
  page,
}) => {
  await viewer.canvas.focus();
  const pad = xbox();
  pad.buttons[0] = 1;
  await setGamepads(page, [null, pad]);
  expect((await viewer.status()).active).toBe(false);
  pad.buttons[0] = 0;
  await setGamepads(page, [null, pad]);
  pad.buttons[0] = 1;
  await setGamepads(page, [null, pad]);
  await expect.poll(async () => (await viewer.status()).active).toBe(true);
});

test("unsupported controller mappings are reported without activating", async ({
  viewer,
  page,
}) => {
  await setGamepads(page, [
    xbox({ index: 0, mapping: "", axes: [], buttons: [] }),
  ]);
  await expect
    .poll(async () => (await viewer.status()).gamepadStatus)
    .toContain("Unsupported");
  expect((await viewer.status()).active).toBe(false);
});
