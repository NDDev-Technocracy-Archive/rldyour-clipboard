import { readFile } from "node:fs/promises";
import { createServer, type Server } from "node:http";
import { extname, resolve, sep } from "node:path";
import { expect, test } from "@playwright/test";
import { resources } from "../src/localization";

let server: Server;
let address: string;
test.beforeAll(async () => {
  const root = resolve("dist-preview");
  const types: Record<string, string> = {
    ".html": "text/html",
    ".js": "text/javascript",
    ".css": "text/css",
    ".svg": "image/svg+xml",
    ".woff2": "font/woff2",
  };
  server = createServer(async (request, response) => {
    try {
      const name = decodeURIComponent(
        new URL(request.url ?? "/", "http://localhost").pathname,
      );
      const path = resolve(root, `.${name === "/" ? "/index.html" : name}`);
      if (!path.startsWith(root + sep)) {
        response.writeHead(400).end();
        return;
      }
      response.setHeader(
        "Content-Type",
        types[extname(path)] ?? "application/octet-stream",
      );
      response.end(await readFile(path));
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
  const location = server.address();
  if (!location || typeof location === "string")
    throw new Error("No isolated listener");
  address = `http://127.0.0.1:${location.port}`;
});
test.afterAll(async () => {
  await new Promise<void>((done, reject) =>
    server.close((error) => (error ? reject(error) : done())),
  );
});

for (const language of ["ru", "en", "kk", "ar", "zh"] as const) {
  for (const theme of ["light", "dark", "light-contrast", "dark-contrast"]) {
    test(`${language} ${theme}: history, favorites and exact entry keys`, async ({
      page,
    }, info) => {
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      await page.goto(`${address}/?language=${language}&theme=${theme}`);
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
