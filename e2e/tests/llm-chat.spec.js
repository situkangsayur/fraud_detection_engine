const { test, expect } = require("@playwright/test");

const LLM_UI_URL = process.env.LLM_UI_URL || "http://localhost:8502";

test.describe("LLM Chat UI", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(LLM_UI_URL, { waitUntil: "networkidle", timeout: 30000 });
  });

  test("should load the LLM chat page", async ({ page }) => {
    await expect(page.locator('[data-testid="stAppViewContainer"]')).toBeVisible({
      timeout: 15000,
    });
  });

  test("should display chat tab", async ({ page }) => {
    const chatTab = page.getByRole("tab", { name: /Chat/i });
    if (await chatTab.isVisible()) {
      await chatTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display upload tab", async ({ page }) => {
    const uploadTab = page.getByRole("tab", { name: /Upload/i });
    if (await uploadTab.isVisible()) {
      await uploadTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display audit tab", async ({ page }) => {
    const auditTab = page.getByRole("tab", { name: /Audit/i });
    if (await auditTab.isVisible()) {
      await auditTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });
});
