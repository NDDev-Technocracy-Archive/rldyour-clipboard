import { expect, test } from "@playwright/test";
import { createPreviewServer } from "../preview/server";
import { resources } from "../src/localization";

let server: Awaited<ReturnType<typeof createPreviewServer>> | undefined;
let address: string;
test.beforeAll(async () => {
  server = await createPreviewServer();
  address = server.url;
});
test.afterAll(async () => {
  if (server) await server.close();
});

for (const language of ["ru", "en", "kk", "ar", "zh"] as const) {
  for (const theme of ["light", "dark", "light-contrast", "dark-contrast"]) {
    test(`${language} ${theme}: history, favorites and exact entry keys`, async ({
      page,
    }, info) => {
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      await page.goto(`${address}/?language=${language}&theme=${theme}`);
      const fonts = await page.evaluate(async () => {
        await document.fonts.ready;
        const faces = Array.from(document.fonts);
        return {
          loaded: faces.filter((face) => face.status === "loaded").length,
          failed: faces
            .filter((face) => face.status === "error")
            .map((face) => face.family),
        };
      });
      expect(fonts.loaded).toBeGreaterThan(0);
      expect(fonts.failed).toEqual([]);
      const labels = resources[language];
      await expect(page.getByRole("searchbox")).toBeVisible();
      await expect(page.locator(".entries-list > li")).toHaveCount(4);
      await page
        .getByRole("tab", { name: labels.favorites, exact: true })
        .click();
      await expect(page.locator(".entries-list > li")).toHaveCount(2);
      await page.locator(".entry-restore").first().click();
      await expect(page.locator("html")).toHaveAttribute(
        "data-restored-key",
        "9223372036854775807",
      );
      await page
        .getByRole("tab", { name: labels.history, exact: true })
        .click();
      await page
        .getByRole("button", { name: labels.more, exact: true })
        .click();
      await expect(page.locator(".entries-list > li")).toHaveCount(6);
      await expect(page.locator(".entry-preview").last()).toContainText(
        "<script>",
      );
      expect(
        await page.evaluate(() =>
          Reflect.get(window, "clipboardScriptExecuted"),
        ),
      ).toBeUndefined();
      await page.getByRole("searchbox").fill("Фотография");
      await expect(page.locator(".entries-list > li")).toHaveCount(1);
      await page.locator(".entry-pin").click();
      await expect(page.locator(".entry-pin")).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      await page.getByRole("searchbox").fill("");
      await page.setViewportSize({ width: 360, height: 640 });
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({
        path: info.outputPath(`${language}-${theme}.png`),
      });
      const layout = await page.evaluate(() => ({
        width: document.documentElement.scrollWidth,
        outside: [...document.querySelectorAll("body *")]
          .map((element) => ({
            tag: element.tagName,
            class: element.className,
            left: element.getBoundingClientRect().left,
            right: element.getBoundingClientRect().right,
          }))
          .filter((element) => element.left < 0 || element.right > innerWidth),
      }));
      if (layout.width !== 360)
        await info.attach("overflow.json", {
          body: JSON.stringify(layout, null, 2),
          contentType: "application/json",
        });
      expect(layout.width).toBe(360);
      expect(errors).toEqual([]);
    });
  }
}

test("empty, unavailable and keyboard navigation", async ({ page }) => {
  await page.goto(`${address}/?empty`);
  await expect(
    page.getByText(resources.ru.empty_title, { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("tab", { name: resources.ru.recent, exact: true })
    .focus();
  await page.keyboard.press("ArrowRight");
  await expect(
    page.getByRole("tab", { name: resources.ru.history, exact: true }),
  ).toBeFocused();
  await page.keyboard.press("End");
  await expect(
    page.getByRole("tab", { name: resources.ru.favorites, exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Home");
  await expect(
    page.getByRole("tab", { name: resources.ru.recent, exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("main")).toHaveCount(0);
  await page.goto(`${address}/?failure`);
  await expect(page.getByRole("alert")).toContainText(resources.ru.error_title);
  await expect(
    page.getByRole("button", { name: resources.ru.retry, exact: true }),
  ).toBeVisible();
});
