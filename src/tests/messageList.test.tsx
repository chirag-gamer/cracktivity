/**
 * L1: virtualized list sanity (NFR-1) + row rendering + selection.
 * jsdom has no layout, so the virtualizer renders only the initial window
 * plus overscan — which is exactly what we assert: never all rows.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { MessageList } from "../main/MessageList";
import { thread } from "./fixtures";

afterEach(cleanup);

describe("MessageList", () => {
  it("renders only a window of 10k rows (virtualization works)", () => {
    const rows = Array.from({ length: 10_000 }, (_, i) => thread(i));
    const { getAllByTestId } = render(
      <MessageList threads={rows} selectedThrid={null} onSelect={() => {}} loading={false} />,
    );
    const rendered = getAllByTestId("thread-row").length;
    expect(rendered).toBeGreaterThan(0);
    // A 10k list must never render every row.
    expect(rendered).toBeLessThan(100);
  });

  it("highlights the selected row and reports clicks", () => {
    const onSelect = vi.fn();
    const { getAllByTestId } = render(
      <MessageList
        threads={[thread(1), thread(2)]}
        selectedThrid="t1"
        onSelect={onSelect}
        loading={false}
      />,
    );
    const rows = getAllByTestId("thread-row");
    expect(rows[0].getAttribute("aria-selected")).toBe("true");
    expect(rows[1].getAttribute("aria-selected")).toBe("false");
    fireEvent.click(rows[0]);
    expect(onSelect).toHaveBeenCalledWith("t1");
  });

  it("shows the empty state for zero threads", () => {
    const { getByRole } = render(
      <MessageList threads={[]} selectedThrid={null} onSelect={() => {}} loading={false} />,
    );
    expect(getByRole("status").textContent).toContain("No mail");
  });
});
