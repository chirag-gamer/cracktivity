/**
 * First-run setup wizard (FR-20/D-005, restyled per test/ui-redesign-notes.md):
 * screen 1 is the AI opt-in (checkbox ON by default, cost stated plainly,
 * single accept records completion), then an optional look-and-feel step and
 * the first-account step. Declining AI leaves zero model files on disk.
 * Step order follows the research in test/ui-redesign-notes.md section 8:
 * four short screens, every one skippable except the AI choice.
 */
import { useState } from "react";
import { Checkbox } from "@fluentui/react-components";
import { Mail, ShieldCheck, Sparkles, Zap } from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";
import { useThemeChoice, type ThemeChoice } from "../shared/theme";

export interface OnboardingProps {
  /** True when `onboarding.completed` is unset (fresh install). */
  open: boolean;
  onFinish: () => void;
  /** Opens the add-account dialog over the wizard. */
  onAddAccount: () => void;
}

type Step = "ai" | "look" | "account";

export function OnboardingDialog({ open, onFinish, onAddAccount }: OnboardingProps) {
  const [enabled, setEnabled] = useState(true);
  const [downloading, setDownloading] = useState(false);
  const [step, setStep] = useState<Step>("ai");
  const { choice, setChoice } = useThemeChoice();

  if (!open) return null;

  const accept = () => {
    if (enabled) {
      setDownloading(true);
      ipc
        .aiSetEnabled(true)
        .then(() => ipc.aiDownloadModel(() => {}, () => {}))
        .catch(() => {})
        .finally(() => {
          ipc.setSetting("onboarding.completed", true).catch(() => {});
          setDownloading(false);
          setStep("look");
        });
    } else {
      ipc.aiSetEnabled(false).catch(() => {});
      ipc.setSetting("onboarding.completed", true).catch(() => {});
      setStep("look");
    }
  };

  return (
    <div className="overlay" role="dialog" aria-modal data-testid="onboarding">
      <div className="ob-card" style={{ maxWidth: 560 }}>
        <div className="ob-progress" aria-hidden>
          <i style={{ width: step === "ai" ? "25%" : step === "look" ? "55%" : "90%" }} />
        </div>

        {step === "ai" && (
          <>
            <div className="ob-eyebrow">{S.onboardingAiTitle}</div>
            <div className="ob-logo" aria-hidden>
              <Sparkles size={22} />
            </div>
            <h2 className="ob-title">{S.onboardingAiTitle}</h2>
            <p className="ob-lead">{S.onboardingAiBody}</p>
            <div className="ob-panel">
              <Checkbox
                label={S.onboardingAccept}
                checked={enabled}
                data-testid="onboarding-ai-checkbox"
                onChange={(_, d) => setEnabled(d.checked === true)}
              />
            </div>
            {downloading ? (
              <div role="status" data-testid="onboarding-downloading" className="ob-downloading">
                {S.aiModelDownloading}
              </div>
            ) : (
              <div className="ob-actions">
                <button className="primary-button" onClick={accept} data-testid="onboarding-accept">
                  {enabled ? S.onboardingAccept : S.onboardingDecline}
                </button>
              </div>
            )}
            <p className="ob-footnote">
              <ShieldCheck size={13} /> {S.onboardingPrivacy}
            </p>
          </>
        )}

        {step === "look" && (
          <>
            <div className="ob-eyebrow">{S.onboardingLookTitle}</div>
            <h2 className="ob-title">{S.onboardingLookTitle}</h2>
            <p className="ob-lead">{S.onboardingLookBody}</p>
            <div className="ob-themes">
              {(
                [
                  { c: "dark" as ThemeChoice, label: S.themeDark, bg: "#0c0c0e", chip: "#17171a" },
                  { c: "light" as ThemeChoice, label: S.themeLight, bg: "#eef0f4", chip: "#ffffff" },
                ]
              ).map((t) => (
                <button
                  key={t.c}
                  className={`ob-theme${choice === t.c ? " picked" : ""}`}
                  onClick={() => setChoice(t.c)}
                  data-testid={`onboarding-theme-${t.c}`}
                >
                  <span className="ob-theme-prev" style={{ background: t.bg }}>
                    <span style={{ background: t.chip }} />
                  </span>
                  <span className="ob-theme-nm">{t.label}</span>
                </button>
              ))}
            </div>
            <div className="ob-actions">
              <button className="primary-button" onClick={() => setStep("account")}>
                {S.onboardingNext}
              </button>
            </div>
          </>
        )}

        {step === "account" && (
          <>
            <div className="ob-eyebrow">{S.addAccountTitle}</div>
            <div className="ob-logo" aria-hidden>
              <Mail size={22} />
            </div>
            <h2 className="ob-title">{S.addAccountTitle}</h2>
            <p className="ob-lead">{S.onboardingAccountBody}</p>
            <div className="ob-actions">
              <button className="primary-button" onClick={onAddAccount} data-testid="onboarding-add-account">
                <Zap size={14} /> {S.addAccount}
              </button>
              <button className="link-button" onClick={onFinish} data-testid="onboarding-finish">
                {S.onboardingSkip}
              </button>
            </div>
            <p className="ob-footnote">
              <Sparkles size={13} /> {S.onboardingDoneHint}
            </p>
          </>
        )}
      </div>
    </div>
  );
}
