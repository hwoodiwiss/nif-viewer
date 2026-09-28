import { expect } from "@playwright/test";

export async function installGamepads(page) {
  await page.addInitScript(() => {
    window.testGamepads = { pads: [], revision: 0, observed: -1, polls: 0 };
    const platformObject = (prototype, values) =>
      Object.create(
        prototype,
        Object.fromEntries(
          Object.entries(values).map(([key, value]) => [key, { value }]),
        ),
      );
    Object.defineProperty(navigator, "getGamepads", {
      value: () => {
        const state = window.testGamepads;
        state.polls = state.observed === state.revision ? state.polls + 1 : 1;
        state.observed = state.revision;
        return state.pads.map(
          (pad) =>
            pad &&
            platformObject(Gamepad.prototype, {
              ...pad,
              buttons: pad.buttons.map((value) =>
                platformObject(GamepadButton.prototype, {
                  value,
                  pressed: value > 0.5,
                  touched: value > 0,
                }),
              ),
            }),
        );
      },
    });
  });
}

export function xbox(changes = {}) {
  return {
    id: "Test Xbox",
    index: 1,
    connected: true,
    mapping: "standard",
    axes: [0, 0, 0, 0],
    buttons: Array(16).fill(0),
    ...changes,
  };
}

export async function setGamepads(page, pads) {
  const revision = await page.evaluate((pads) => {
    window.testGamepads.pads = pads;
    return ++window.testGamepads.revision;
  }, pads);
  await expect
    .poll(
      () =>
        page.evaluate((revision) => {
          const state = window.testGamepads;
          return state.observed === revision && state.polls >= 2;
        }, revision),
      { message: "WASM has polled the new controller state" },
    )
    .toBe(true);
}
