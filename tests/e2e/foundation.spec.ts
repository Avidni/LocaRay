import { expect, test } from "@playwright/test";

test("starts from a detected local service", async ({ page }) => {
  await page.goto("/");

  await expect(
    page.getByRole("heading", { name: "Share your local app" }),
  ).toBeVisible();
  await expect(
    page.getByRole("spinbutton", { name: "Local port" }),
  ).toHaveValue("5173");
  await expect(
    page.getByRole("button", { name: "Start tunnel" }),
  ).toBeEnabled();
  await expect(
    page.getByText("Anyone with the generated URL may access this local app."),
  ).toBeVisible();
});

test("renders the complete connected session", async ({ page }) => {
  await page.goto("/?state=connected");

  await expect(
    page.getByRole("heading", { name: "Your app is live" }),
  ).toBeVisible();
  await expect(
    page.getByText("https://calm-river-5173.trycloudflare.com"),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Copy URL" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Open", exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("QR code for the public URL")).toBeVisible();
  await expect(page.getByRole("button", { name: "Stop tunnel" })).toBeVisible();
});

test("resets an origin error before changing ports", async ({ page }) => {
  await page.goto("/?state=error");

  await expect(
    page.getByRole("heading", { name: "Nothing is running on port 5173." }),
  ).toBeFocused();
  await page.getByRole("button", { name: "Change port" }).click();
  await expect(
    page.getByRole("spinbutton", { name: "Local port" }),
  ).toHaveValue("");
  await expect(
    page.getByRole("button", { name: "Start tunnel" }),
  ).toBeDisabled();
});

test("traps focus in the active-tunnel close confirmation", async ({
  page,
}) => {
  await page.goto("/?state=connected&close=1");

  const stopAndQuit = page.getByRole("button", { name: "Stop and quit" });
  await expect(stopAndQuit).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "Cancel" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(stopAndQuit).toBeFocused();
});

test("restores settings focus and has no minimum-width overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 560 });
  await page.goto("/");

  const settings = page.getByRole("button", { name: "Open settings" });
  await settings.click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(settings).toBeFocused();

  const hasHorizontalOverflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth >
      document.documentElement.clientWidth,
  );
  expect(hasHorizontalOverflow).toBe(false);
});
