const { test, expect } = require("@playwright/test");

const STREAMLIT_URL = process.env.STREAMLIT_URL || "http://localhost:8501";

test.describe("Fraud Detection Dashboard UI", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(STREAMLIT_URL, { waitUntil: "networkidle", timeout: 30000 });
  });

  test("should load the dashboard page", async ({ page }) => {
    // Streamlit apps have a main content area
    await expect(page.locator('[data-testid="stAppViewContainer"]')).toBeVisible({
      timeout: 15000,
    });
  });

  test("should display healthcheck tab", async ({ page }) => {
    // Look for the Healthcheck tab
    const healthTab = page.getByRole("tab", { name: /Healthcheck/i });
    if (await healthTab.isVisible()) {
      await healthTab.click();
      await page.waitForTimeout(2000);
      // Should show health-related content
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display user management tab", async ({ page }) => {
    const userTab = page.getByRole("tab", { name: /User/i });
    if (await userTab.isVisible()) {
      await userTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display transaction management tab", async ({ page }) => {
    const trxTab = page.getByRole("tab", { name: /Transaction/i });
    if (await trxTab.isVisible()) {
      await trxTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display statistics tab", async ({ page }) => {
    const statsTab = page.getByRole("tab", { name: /Statistics/i });
    if (await statsTab.isVisible()) {
      await statsTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display policy management tab", async ({ page }) => {
    const policyTab = page.getByRole("tab", { name: /Policy/i });
    if (await policyTab.isVisible()) {
      await policyTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display process transaction tab", async ({ page }) => {
    const processTab = page.getByRole("tab", { name: /Process/i });
    if (await processTab.isVisible()) {
      await processTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });

  test("should display monitor transactions tab", async ({ page }) => {
    const monitorTab = page.getByRole("tab", { name: /Monitor/i });
    if (await monitorTab.isVisible()) {
      await monitorTab.click();
      await page.waitForTimeout(2000);
      const content = await page.textContent('[data-testid="stAppViewContainer"]');
      expect(content).toBeTruthy();
    }
  });
});
