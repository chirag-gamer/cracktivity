/**
 * V-3.1 v1 (seeded, offline): onboarding seed → unified list → keyboard
 * triage → reader → FTS search. Native-window assertions (Alt-Tab exclusion,
 * hotkey-under-focus) are L6/FlaUI work; send/reply is the Phase-3 compose
 * follow-up.
 */
describe("cracktivity inbox (e2e seed)", () => {
  it("hydrates the unified inbox from the local encrypted cache", async () => {
    await $('[data-testid="nav-unified"]').waitForDisplayed({ timeout: 20_000 });
    await $('[data-testid="thread-row"]').waitForDisplayed({ timeout: 20_000 });
    const rows = await $$('[data-testid="thread-row"]');
    expect(rows.length).toBe(5);
  });

  it("keyboard triage: j selects and the reader opens", async () => {
    await browser.keys("j");
    await $('[data-testid="reader"]').waitForDisplayed({ timeout: 10_000 });
    const cards = await $$('[data-testid="msg-card"]');
    expect(cards.length).toBeGreaterThanOrEqual(1);
  });

  it("Ctrl+1 switches to the first account view (FR-4)", async () => {
    await browser.keys(["Control", "1", "Control"]);
    await $('[data-testid="nav-account-1"].active').waitForExist({ timeout: 10_000 });
  });

  it("back to unified, / searches the cache via FTS5 (FR-9 local)", async () => {
    await $('[data-testid="nav-unified"]').click();
    // "/" focuses the search input (FR-30); typing goes straight into it.
    // (element.click() on the Fluent input hangs the native WebView2 driver —
    // the keyboard path is also the more honest e2e anyway.)
    await browser.keys("/");
    await browser.keys("451902");
    await $('[data-testid="search-results"]').waitForDisplayed({ timeout: 10_000 });
    const hits = await $$('[data-testid="search-results"] .row');
    expect(hits.length).toBe(1);
  });
});
