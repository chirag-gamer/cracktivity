/**
 * V-5.1 (Phase 5): keyboard-only path into Settings, accessible appearance
 * controls, and a theme switch that repaints the document (D-040).
 */
describe("cracktivity settings (e2e, keyboard-first)", () => {
  it("reaches Settings through the command palette (Ctrl+K)", async () => {
    await $('[data-testid="nav-unified"]').waitForDisplayed({ timeout: 20_000 });
    await browser.keys(["Control", "k", "Control"]);
    await $('[data-testid="palette-input"]').waitForDisplayed({ timeout: 10_000 });
    await browser.keys("settings");
    await browser.keys("Enter");
    await $('[data-testid="settings-view"]').waitForDisplayed({ timeout: 10_000 });
  });

  it("exposes the appearance theme select with a real label (a11y)", async () => {
    const theme = await $("select[data-testid='theme-choice']");
    await theme.waitForExist({ timeout: 10_000 });
    const id = await theme.getAttribute("id");
    expect(id).toBe("theme-choice");
    const labels = await $$('label[for="theme-choice"]');
    expect(labels.length).toBe(1);
  });

  it("moves keyboard focus into the appearance controls (V-5.1)", async () => {
    await browser.execute(() => {
      (
        document.querySelector("select[data-testid='theme-choice']") as HTMLElement
      ).focus();
    });
    await browser.keys("Tab");
    await browser.pause(200);
    const focused = await browser.execute(
      () =>
        document.activeElement?.getAttribute("data-testid") ??
        document.activeElement?.tagName ??
        "",
    );
    // Theme select → keymap select: keyboard focus travel works.
    expect(focused).toBe("keymap-choice");
  });

  it("switching the theme repaints the document (D-040)", async () => {
    // Synthesized keys do not drive native <select> change events on
    // WebView2 (untrusted-input gate, webdriverio/desktop-mobile#612), so
    // the flip is driven by dispatch and the repaint contract is asserted.
    await browser.execute(() => {
      const el = document.querySelector(
        "select[data-testid='theme-choice']",
      ) as HTMLSelectElement;
      el.value = "dark";
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await browser.pause(500);
    const docTheme = await browser.execute(() =>
      document.documentElement.getAttribute("data-theme"),
    );
    expect(docTheme).toBe("dark");
  });
});
