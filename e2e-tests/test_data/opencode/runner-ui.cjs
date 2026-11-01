const path = require("node:path");
const { chromium, expect } = require(
  require.resolve("@playwright/test", {
    paths: [path.join(process.argv[2], "web")],
  }),
);
(async () => {
  const origin = process.argv[3];
  const browser = await chromium.launch({ channel: "chrome", headless: true });
  try {
    const context = await browser.newContext({
      viewport: { width: 1280, height: 900 },
      colorScheme: "light",
    });
    await context.addInitScript(() =>
      localStorage.setItem(
        "bifrost-locale",
        JSON.stringify({ state: { locale: "en" }, version: 0 }),
      ),
    );
    const page = await context.newPage();
    page.setDefaultTimeout(10000);
    await page.goto(`${origin}/_bifrost/ai/agents`);
    console.log("UI: Agent page opened");
    const row = page.getByRole("row").filter({ hasText: "OpenCode" });
    await expect(row).toHaveCount(1);
    for (const theme of ["light", "dark"]) {
      if ((await page.locator("html").getAttribute("data-theme")) !== theme)
        await page.getByTestId("theme-toggle").click();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await row.getByRole("button", { name: /Edit/ }).click();
      const dialog = page.getByRole("dialog");
      console.log("UI: editing", theme);
      await expect(
        dialog.getByLabel("Default model", { exact: true }),
      ).toHaveValue("opencode/test");
      await expect(dialog.getByLabel("Adapter", { exact: true })).toBeVisible();
      await expect(dialog.getByLabel("Arguments", { exact: true })).toHaveCount(
        0,
      );
      await expect(
        dialog.getByLabel("Permission Mode", { exact: true }),
      ).toHaveCount(0);
      // Switching back must not retain fields unsupported by OpenCode.
      await dialog.getByLabel("Adapter", { exact: true }).click();
      await page.getByText("Codex CLI", { exact: true }).last().click();
      await dialog
        .getByLabel("Arguments", { exact: true })
        .fill("--unsafe-test-argument");
      await dialog.getByLabel("Permission Mode", { exact: true }).click();
      await page.getByText("Plan", { exact: true }).last().click();
      await dialog.getByLabel("Adapter", { exact: true }).click();
      await page.getByText("OpenCode", { exact: true }).last().click();
      await expect(dialog.getByLabel("Arguments", { exact: true })).toHaveCount(
        0,
      );
      await expect(
        dialog.getByLabel("Permission Mode", { exact: true }),
      ).toHaveCount(0);
      await dialog
        .getByLabel("Default model", { exact: true })
        .fill("opencode/changed");
      await dialog
        .getByRole("button", { name: "Save Runner", exact: true })
        .click();
      await expect(dialog).not.toBeVisible();
      const config = await (
        await page.request.get(`${origin}/_bifrost/api/im-gateway/chat/config`)
      ).json();
      expect(config.runners.OpenCode.adapterConfig.model).toBe(
        "opencode/changed",
      );
      expect(config.runners.OpenCode.adapterConfig.timeoutSecs).toBe(10);
      expect(config.runners.OpenCode.adapterConfig.args ?? []).toEqual([]);
      expect(
        config.runners.OpenCode.adapterConfig.permissionMode ?? "default",
      ).toBe("default");
      await row.getByRole("button", { name: /Edit/ }).click();
      await expect(
        dialog.getByLabel("Default model", { exact: true }),
      ).toHaveValue("opencode/changed");
      await dialog
        .getByLabel("Default model", { exact: true })
        .fill("opencode/test");
      await dialog
        .getByRole("button", { name: "Save Runner", exact: true })
        .click();
      await expect(dialog).not.toBeVisible();
    }
    console.log(
      "PASS: OpenCode default model save/reopen, existing configuration preserved, light/dark UI",
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
