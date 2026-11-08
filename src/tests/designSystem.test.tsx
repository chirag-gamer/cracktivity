/**
 * L1: Design System primitives, LucideProvider icon sizing/stroke invariants,
 * 5-state button behavior, 3-step onboarding wizard navigation, and first-run
 * / quick-reply action handlers (Phase 8 / D-049).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { Mail } from "lucide-react";
import { ThemeProvider } from "../shared/theme";
import {
  Avatar,
  BadgePill,
  EmptyStateCard,
  IconButton,
  Kbd,
  StatusDot,
  ToastBanner,
  UiButton,
  extractInitials,
  hashAvatarSlot,
} from "../shared/ui";
import { AccountPane } from "../main/AccountPane";
import { MessageList } from "../main/MessageList";
import { Reader } from "../main/Reader";
import { OnboardingDialog } from "../main/Onboarding";
import { AddAccountDialog } from "../main/AddAccountDialog";
import { message, thread } from "./fixtures";

beforeEach(() => {
  mockIPC((cmd) => {
    switch (cmd) {
      case "get_settings":
        return {};
      case "set_setting":
      case "ai_set_enabled":
      case "ai_download_model":
        return null;
      case "message_body":
        return {
          text: "body text",
          html: null,
          blocked_images: 0,
          tracking_count: 0,
          snippet: "body text",
        };
      default:
        return null;
    }
  });
});

afterEach(() => {
  clearMocks();
  cleanup();
});

describe("Design System Primitives (src/shared/ui.tsx)", () => {
  it("extracts initials and deterministic WCAG-compliant avatar slots", () => {
    expect(extractInitials("Ada Lovelace <ada@analytical.org>")).toBe("AL");
    expect(extractInitials("grace.hopper@navy.mil")).toBe("GH");
    expect(extractInitials("single")).toBe("SI");
    const slotA = hashAvatarSlot("ada@analytical.org");
    const slotB = hashAvatarSlot("ADA@analytical.org");
    expect(slotA).toEqual(slotB);
    expect(slotA.fg).toBe("#ffffff");
  });

  it("UiButton handles click, disabled, loading, and kbd shortcut states", () => {
    const onClick = vi.fn();
    const { getByRole, rerender } = render(
      <UiButton variant="primary" size="md" pill kbd="C" onClick={onClick}>
        Compose
      </UiButton>,
    );
    const btn = getByRole("button");
    expect(btn.className).toContain("ui-btn--primary");
    expect(btn.className).toContain("ui-btn--pill");
    expect(btn.querySelector("kbd")?.textContent).toBe("C");

    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledTimes(1);

    rerender(
      <UiButton variant="primary" disabled onClick={onClick}>
        Compose
      </UiButton>,
    );
    expect((getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(getByRole("button"));
    expect(onClick).toHaveBeenCalledTimes(1);

    rerender(
      <UiButton variant="primary" loading onClick={onClick}>
        Compose
      </UiButton>,
    );
    expect(getByRole("button").getAttribute("aria-busy")).toBe("true");
    expect((getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  });

  it("renders IconButton, BadgePill, StatusDot, Kbd, EmptyStateCard, and ToastBanner", () => {
    const onAction = vi.fn();
    const onDismiss = vi.fn();
    const { getByLabelText, getByTestId } = render(
      <div>
        <IconButton label="Sync inbox" badge={3}>
          <Mail />
        </IconButton>
        <BadgePill variant="ai">AI Ready</BadgePill>
        <StatusDot state="connected" showLabel />
        <Kbd>Ctrl+K</Kbd>
        <EmptyStateCard
          testId="empty-card"
          eyebrow="GETTING STARTED"
          title="Welcome"
          description="Connect an account"
        />
        <ToastBanner
          testId="toast"
          message="Archived 1 thread"
          actionLabel="Undo"
          onAction={onAction}
          onDismiss={onDismiss}
        />
        <Avatar name="Jane Doe" status="connected" ring />
      </div>,
    );

    expect(getByLabelText("Sync inbox").textContent).toContain("3");
    expect(getByTestId("empty-card").textContent).toContain("Welcome");
    const toast = getByTestId("toast");
    fireEvent.click(toast.querySelector("button")!);
    expect(onAction).toHaveBeenCalledTimes(1);
  });
});

describe("Icon System & Stroke Consistency (ThemeProvider + LucideProvider)", () => {
  it("applies default size=15 and strokeWidth=1.75 via LucideProvider and never fills Mail", () => {
    const { container } = render(
      <ThemeProvider>
        <AccountPane
          accounts={[]}
          statuses={[]}
          scope={{ kind: "unified" }}
          onScope={() => {}}
        />
      </ThemeProvider>,
    );
    const svgs = Array.from(container.querySelectorAll("svg"));
    expect(svgs.length).toBeGreaterThan(0);
    for (const svg of svgs) {
      expect(svg.getAttribute("width")).toBe("15");
      expect(svg.getAttribute("height")).toBe("15");
      expect(svg.getAttribute("stroke-width")).toBe("1.75");
      // Outline icons in AccountPane must never have fill="currentColor"
      expect(svg.getAttribute("fill")).not.toBe("currentColor");
    }
  });
});

describe("Onboarding Wizard & First-Run Experience", () => {
  it("walks through AI -> Appearance (Dark/Light/System) -> Account steps with Back navigation", async () => {
    const onFinish = vi.fn();
    const onAddAccount = vi.fn();
    let downloadCalls = 0;
    mockIPC((cmd) => {
      if (cmd === "ai_download_model") {
        downloadCalls += 1;
      }
      return null;
    });

    const { getByTestId, getByText } = render(
      <ThemeProvider>
        <OnboardingDialog
          open
          onFinish={onFinish}
          onAddAccount={onAddAccount}
        />
      </ThemeProvider>,
    );

    // Step 1 -> Step 2
    fireEvent.click(getByTestId("onboarding-accept"));
    await waitFor(() =>
      expect(getByTestId("onboarding-theme-dark")).toBeInTheDocument(),
    );
    expect(downloadCalls).toBe(1);
    expect(getByTestId("onboarding-theme-light")).toBeInTheDocument();
    expect(getByTestId("onboarding-theme-system")).toBeInTheDocument();

    // Exercise Back from Step 2 -> Step 1, and ensure accepting again does not re-download
    fireEvent.click(getByText("Back"));
    await waitFor(() =>
      expect(getByTestId("onboarding-accept")).toBeInTheDocument(),
    );
    fireEvent.click(getByTestId("onboarding-accept"));
    await waitFor(() =>
      expect(getByTestId("onboarding-theme-dark")).toBeInTheDocument(),
    );
    expect(downloadCalls).toBe(1);

    fireEvent.click(getByTestId("onboarding-theme-dark"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");

    // Step 2 -> Step 3
    fireEvent.click(getByText("Continue"));
    await waitFor(() =>
      expect(getByTestId("onboarding-add-account")).toBeInTheDocument(),
    );

    // Exercise Back from Step 3 -> Step 2 -> Step 3
    fireEvent.click(getByText("Back"));
    await waitFor(() =>
      expect(getByTestId("onboarding-theme-dark")).toBeInTheDocument(),
    );
    fireEvent.click(getByText("Continue"));
    await waitFor(() =>
      expect(getByTestId("onboarding-add-account")).toBeInTheDocument(),
    );

    fireEvent.click(getByTestId("onboarding-add-account"));
    expect(onAddAccount).toHaveBeenCalledTimes(1);

    fireEvent.click(getByTestId("onboarding-finish"));
    expect(onFinish).toHaveBeenCalledTimes(1);
  });

  it("AddAccountDialog quick provider pills switch the active provider", () => {
    const { getByText, queryByTestId } = render(
      <ThemeProvider>
        <AddAccountDialog open onClose={() => {}} onAdded={() => {}} />
      </ThemeProvider>,
    );
    expect(queryByTestId("account-host")).not.toBeInTheDocument();
    expect(queryByTestId("account-smtp-host")).not.toBeInTheDocument();
    fireEvent.click(getByText("Other (IMAP)"));
    expect(queryByTestId("account-host")).toBeInTheDocument();
    expect(queryByTestId("account-smtp-host")).toBeInTheDocument();
  });

  it("MessageList first-run empty state triggers onAddAccount and row hover actions work", () => {
    const onAddAccount = vi.fn();
    const onArchiveThread = vi.fn();
    const onToggleStarThread = vi.fn();

    const { getByText, rerender, getAllByLabelText } = render(
      <MessageList
        threads={[]}
        selectedThrid={null}
        onSelect={() => {}}
        loading={false}
        hasAccounts={false}
        onAddAccount={onAddAccount}
      />,
    );

    fireEvent.click(getByText("Add account"));
    expect(onAddAccount).toHaveBeenCalledTimes(1);

    rerender(
      <MessageList
        threads={[thread(1)]}
        selectedThrid="t1"
        onSelect={() => {}}
        loading={false}
        hasAccounts
        onArchiveThread={onArchiveThread}
        onToggleStarThread={onToggleStarThread}
      />,
    );

    fireEvent.click(getAllByLabelText("Archive thread")[0]!);
    expect(onArchiveThread).toHaveBeenCalledWith("t1");
    fireEvent.click(getAllByLabelText("Star thread")[0]!);
    expect(onToggleStarThread).toHaveBeenCalledWith("t1", true);
  });

  it("Reader empty state and thread quick-reply / toolbar buttons invoke callbacks directly", () => {
    const onCompose = vi.fn();
    const onReply = vi.fn();
    const onArchiveThread = vi.fn();
    const onToggleStarThread = vi.fn();
    const onDraftReply = vi.fn();

    const { getByText, getByTitle, rerender } = render(
      <Reader
        messages={[]}
        onMarkRead={() => {}}
        aiEnabled
        threadId={null}
        onDraftReply={onDraftReply}
        onCompose={onCompose}
      />,
    );

    fireEvent.click(getByText("Compose"));
    expect(onCompose).toHaveBeenCalledTimes(1);

    rerender(
      <Reader
        messages={[message(1)]}
        onMarkRead={() => {}}
        aiEnabled
        threadId="t1"
        onDraftReply={onDraftReply}
        onReply={onReply}
        onArchiveThread={onArchiveThread}
        onToggleStarThread={onToggleStarThread}
      />,
    );

    fireEvent.click(getByTitle("Archive thread (e)"));
    expect(onArchiveThread).toHaveBeenCalledTimes(1);

    fireEvent.click(getByTitle("Star thread (s)"));
    expect(onToggleStarThread).toHaveBeenCalledTimes(1);

    fireEvent.click(getByText("Reply"));
    expect(onReply).toHaveBeenCalledTimes(1);

    fireEvent.click(getByText("Sounds good, thanks!"));
    expect(onDraftReply).toHaveBeenCalledWith("t1", "Sounds good, thanks!");
  });
});
