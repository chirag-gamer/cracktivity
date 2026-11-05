/**
 * First-run onboarding (FR-20/D-005): the AI opt-in dialog shows exactly
 * once (checkbox ON by default), states the cost plainly, and flows into
 * the first-account step. Declining leaves zero model files on disk.
 */
import { useState } from "react";
import { Checkbox, Text } from "@fluentui/react-components";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";

export interface OnboardingProps {
  /** True when `onboarding.completed` is unset (fresh install). */
  open: boolean;
  onFinish: () => void;
  /** Opens the add-account dialog after the AI choice. */
  onAddAccount: () => void;
}

export function OnboardingDialog({ open, onFinish, onAddAccount }: OnboardingProps) {
  const [enabled, setEnabled] = useState(true);
  const [phase, setPhase] = useState<"ai" | "downloading">("ai");

  if (!open) return null;

  const accept = () => {
    if (enabled) {
      setPhase("downloading");
      ipc
        .aiSetEnabled(true)
        .then(() => ipc.aiDownloadModel(() => {}, () => {}))
        .catch(() => {})
        .finally(() => {
          ipc.setSetting("onboarding.completed", true).catch(() => {});
          onAddAccount();
          onFinish();
        });
    } else {
      ipc.aiSetEnabled(false).catch(() => {});
      ipc.setSetting("onboarding.completed", true).catch(() => {});
      onAddAccount();
      onFinish();
    }
  };

  return (
    <div
      className="overlay"
      role="dialog"
      aria-modal
      aria-label={S.onboardingAiTitle}
      data-testid="onboarding"
    >
      <div className="overlay-card">
        <h2>{S.onboardingAiTitle}</h2>
        <Text>{S.onboardingAiBody}</Text>
        {phase === "ai" ? (
          <>
            <Checkbox
              label={S.onboardingAccept}
              checked={enabled}
              data-testid="onboarding-ai-checkbox"
              onChange={(_, d) => setEnabled(d.checked === true)}
            />
            <div className="overlay-actions">
              <button className="primary-button" onClick={accept} data-testid="onboarding-accept">
                {enabled ? S.onboardingAccept : S.onboardingDecline}
              </button>
            </div>
          </>
        ) : (
          <div role="status" data-testid="onboarding-downloading">
            {S.aiModelDownloading}
          </div>
        )}
      </div>
    </div>
  );
}
