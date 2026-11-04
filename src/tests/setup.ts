import "@testing-library/jest-dom/vitest";

/**
 * jsdom gives every element clientWidth/clientHeight 0; @tanstack/react-
 * virtual's getRect uses exactly those properties to size the viewport, so
 * without this stub it renders zero rows (verified against virtual-core
 * 3.19.11's observeElementRect source). Test-only; production measures for
 * real. ResizeObserver is stubbed for the later observe path.
 */
Object.defineProperty(HTMLElement.prototype, "clientHeight", {
  configurable: true,
  get(this: HTMLElement) {
    return this.classList.contains("pane-list") ? 768 : 0;
  },
});
Object.defineProperty(HTMLElement.prototype, "clientWidth", {
  configurable: true,
  get(this: HTMLElement) {
    return this.classList.contains("pane-list") ? 480 : 0;
  },
});

class ResizeObserverStub implements ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
