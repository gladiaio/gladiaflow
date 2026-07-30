import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import path from "node:path";

const OUTPUT_DIR =
  process.env.SCREENSHOT_OUTPUT_DIR ??
  path.join(process.env.HOME, "gladiaflow-screenshots");

const screensArg = process.argv[2];
const SCREENS = screensArg
  ? [screensArg]
  : [
      "01-permissions",
      "02-api-key",
      "03-home-idle",
      "04-home-recording",
      "04b-home-api-open",
      "05-history",
      "05b-history-loading",
      "05c-history-empty",
      "06-vocabulary",
      "06b-vocabulary-editor",
      "06c-vocabulary-new",
      "07-transcription-settings",
      "08-app-settings",
      "09-language-picker",
    ];

const baseUrl = process.env.SCREENSHOT_BASE_URL ?? "http://localhost:1420";

async function main() {
  await mkdir(OUTPUT_DIR, { recursive: true });

  const browser = await chromium.launch();
  const page = await browser.newPage({
    viewport: { width: 980, height: 600 },
    deviceScaleFactor: 2,
  });

  await page.goto(`${baseUrl}/screenshots.html`, { waitUntil: "networkidle" });
  await page.waitForTimeout(500);

  const relativeTop = async (screen, target) => {
    const rootSelector = `[id="${screen}"]`;
    const rootBox = await page.locator(rootSelector).boundingBox();
    const targetBox = await page
      .locator(`${rootSelector} ${target}`)
      .boundingBox();
    if (!rootBox || !targetBox) throw new Error(`Unable to measure ${screen}`);
    return Math.round(targetBox.y - rootBox.y);
  };
  const collapsedHomeTitleTop = await relativeTop(
    "03-home-idle",
    ".setup-title",
  );
  const expandedHomeTitleTop = await relativeTop(
    "04b-home-api-open",
    ".setup-title",
  );
  if (collapsedHomeTitleTop !== expandedHomeTitleTop) {
    throw new Error(
      `Home content shifted when opening API key: ${collapsedHomeTitleTop}px -> ${expandedHomeTitleTop}px`,
    );
  }

  const historyFooterTops = await Promise.all(
    ["05-history", "05b-history-loading", "05c-history-empty"].map((screen) =>
      relativeTop(screen, ".history-pagination"),
    ),
  );
  if (new Set(historyFooterTops).size !== 1) {
    throw new Error(
      `History footer moved between states: ${historyFooterTops.join(", ")}`,
    );
  }

  for (const id of SCREENS) {
    const selector = `[id="${id}"]`;
    await page.locator(selector).scrollIntoViewIfNeeded();
    await page.waitForTimeout(200);
    let returnFocusName = null;
    if (id === "06b-vocabulary-editor") {
      returnFocusName = "Edit vocabulary term WebSocket";
      await page
        .locator(selector)
        .getByRole("button", { name: returnFocusName })
        .click();
      await page.locator(".vocab-editor-dialog").waitFor({ state: "visible" });
      await page.waitForTimeout(100);
    }
    if (id === "06c-vocabulary-new") {
      returnFocusName = "Add term";
      await page
        .locator(selector)
        .getByRole("button", { name: returnFocusName })
        .click();
      await page.locator(".vocab-editor-dialog").waitFor({ state: "visible" });
      await page.waitForTimeout(100);
    }
    if (returnFocusName) {
      if (
        (await page
          .locator(".vocab-editor-dialog")
          .getByRole("button", { name: "Cancel", exact: true })
          .count()) !== 0
      ) {
        throw new Error(`${id} still shows a Cancel button`);
      }
      const termHasFocus = await page
        .locator(".vocab-editor-field input")
        .evaluate((element) => element === document.activeElement);
      if (!termHasFocus) {
        throw new Error(`${id} did not initially focus the term input`);
      }
      const pronunciationList = page.locator(".vocab-pronunciation-list");
      const listBox = await pronunciationList.boundingBox();
      const footerBox = await page
        .locator(".vocab-editor-footer")
        .boundingBox();
      if (
        !listBox ||
        !footerBox ||
        listBox.y + listBox.height > footerBox.y - 4
      ) {
        throw new Error(
          `${id} pronunciation list overlaps the dialog footer: ${JSON.stringify({ listBox, footerBox })}`,
        );
      }
      if (id === "06b-vocabulary-editor") {
        const scrollState = await pronunciationList.evaluate((element) => ({
          overflowY: getComputedStyle(element).overflowY,
          clientHeight: element.clientHeight,
          scrollHeight: element.scrollHeight,
        }));
        if (
          scrollState.overflowY !== "auto" ||
          scrollState.scrollHeight <= scrollState.clientHeight
        ) {
          throw new Error(
            `${id} pronunciation list is not scrollable: ${JSON.stringify(scrollState)}`,
          );
        }
      }
    }
    const overflow = await page.locator(selector).evaluate((root) => {
      const selectors = [
        ".layout",
        ".main-content",
        ".content",
        ".setup-step",
        ".home-api-slot",
        ".history-page-list",
        ".vocab-list",
        ".vocab-summary-list",
        ".vocab-editor-dialog",
        ".multi-select-options",
      ];
      return selectors.flatMap((targetSelector) =>
        [...root.querySelectorAll(targetSelector)]
          .filter(
            (element) =>
              element.scrollHeight > element.clientHeight + 1 ||
              element.scrollWidth > element.clientWidth + 1,
          )
          .map((element) => ({
            selector: targetSelector,
            client: [element.clientWidth, element.clientHeight],
            scroll: [element.scrollWidth, element.scrollHeight],
          })),
      );
    });
    if (overflow.length > 0) {
      throw new Error(
        `${id} contains overflowing layout regions: ${JSON.stringify(overflow)}`,
      );
    }
    const outPath = path.join(OUTPUT_DIR, `${id}.png`);
    await page.locator(selector).screenshot({ path: outPath });
    console.log(`Saved ${outPath}`);
    if (returnFocusName) {
      if (id === "06b-vocabulary-editor") {
        const pronunciationInput = page.locator(
          ".vocab-pronunciation-add input",
        );
        await pronunciationInput.fill("WEB SOCKET");
        await page
          .locator(".vocab-pronunciation-add")
          .getByRole("button", { name: "Add" })
          .click();
        await page.getByText("That pronunciation is already listed.").waitFor();
        await page.getByRole("button", { name: "Delete term" }).click();
        await page.getByText("Delete this term?").waitFor();
        await page.getByRole("button", { name: "Keep" }).click();
        await page.locator(".vocab-editor-field input").fill("Unsaved change");
      }
      if (id === "06c-vocabulary-new") {
        await page.locator(".vocab-editor-field input").fill("gladiaflow");
        await page.getByRole("button", { name: "Save term" }).click();
        await page
          .getByText("This term is already in your vocabulary.")
          .waitFor();
        await page.locator(".vocab-editor-field input").fill("Screenshot term");
        await page.getByRole("button", { name: "Save term" }).click();
        await page
          .locator(".vocab-editor-dialog")
          .waitFor({ state: "detached" });
        await page.waitForTimeout(50);
        const savedRow = page.locator(selector).getByRole("button", {
          name: "Edit vocabulary term Screenshot term",
        });
        await savedRow.waitFor();
        await savedRow.click();
        await page
          .locator(".vocab-editor-dialog")
          .waitFor({ state: "visible" });
        await page.getByRole("button", { name: "Delete term" }).click();
        await page.getByRole("button", { name: "Delete" }).click();
        await page
          .locator(".vocab-editor-dialog")
          .waitFor({ state: "detached" });
        if ((await savedRow.count()) !== 0) {
          throw new Error(`${id} did not remove the saved vocabulary term`);
        }
        continue;
      }
      await page.mouse.click(8, 8);
      await page.locator(".vocab-editor-dialog").waitFor({ state: "detached" });
      await page.waitForTimeout(50);
      if (id === "06b-vocabulary-editor") {
        await page
          .locator(selector)
          .getByRole("button", { name: "Edit vocabulary term WebSocket" })
          .waitFor();
      }
      const focusedName = await page.evaluate(
        () =>
          document.activeElement?.getAttribute("aria-label") ??
          document.activeElement?.textContent?.trim(),
      );
      if (focusedName !== returnFocusName) {
        throw new Error(
          `${id} did not restore focus to ${returnFocusName}; focused ${focusedName}`,
        );
      }
    }
  }

  await browser.close();
  console.log(`\nDone — ${SCREENS.length} screenshots in ${OUTPUT_DIR}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
