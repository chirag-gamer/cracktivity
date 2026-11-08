/**
 * First-run setup wizard (FR-20/D-005, Phase 8 overhaul):
 * Step 1: Private On-Device AI opt-in (checkbox ON by default, model specs &
 *         zero-telemetry guarantee shown clearly, single accept records completion).
 * Step 2: Appearance & Theme (Dark, Light, System with live 3-pane mini-previews).
 * Step 3: Connect First Mail Account (provider showcase + Windows Credential
 *         Manager & SQLCipher security summary).
 */
import { useState } from "react";
import { Checkbox } from "@fluentui/react-components";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Cpu,
  HardDrive,
  Lock,
  Mail,
  Monitor,
  Moon,
  ShieldCheck,
  Sparkles,
  Sun,
  Zap,
} from "lucide-react";
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

export function OnboardingDialog({
  open,
  onFinish,
  onAddAccount,
}: OnboardingProps) {
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

  const stepIndex = step === "ai" ? 1 : step === "look" ? 2 : 3;

  return (
    <div className="overlay" role="dialog" aria-modal="true" data-testid="onboarding">
      <div className="ob-card">
        <div className="ob-progress" aria-hidden="true">
          <i
            style={{
              width: step === "ai" ? "33%" : step === "look" ? "66%" : "100%",
            }}
          />
        </div>

        <div className="ob-step-header">
          <div className="ob-step-pills" role="list" aria-label="Setup steps">
            <span
              role="listitem"
              className={`ob-step-pill${step === "ai" ? " is-active" : " is-done"}`}
            >
              <span className="ob-step-num">
                {stepIndex > 1 ? <Check size={10} /> : "1"}
              </span>
              <span>Private AI</span>
            </span>
            <span
              role="listitem"
              className={`ob-step-pill${
                step === "look" ? " is-active" : stepIndex > 2 ? " is-done" : ""
              }`}
            >
              <span className="ob-step-num">
                {stepIndex > 2 ? <Check size={10} /> : "2"}
              </span>
              <span>Appearance</span>
            </span>
            <span
              role="listitem"
              className={`ob-step-pill${step === "account" ? " is-active" : ""}`}
            >
              <span className="ob-step-num">3</span>
              <span>Mail Account</span>
            </span>
          </div>
          <span className="ob-step-counter">Step {stepIndex} of 3</span>
        </div>

        {step === "ai" && (
          <>
            <div className="ob-hero-row">
              <div className="ob-logo" aria-hidden="true">
                <Sparkles size={22} />
              </div>
              <div>
                <div className="ob-eyebrow">LOCAL-FIRST INTELLIGENCE</div>
                <h2 className="ob-title">{S.onboardingAiTitle}</h2>
              </div>
            </div>

            <p className="ob-lead">{S.onboardingAiBody}</p>

            <div className="ob-feature-grid">
              <div className="ob-feature-item">
                <Cpu size={15} className="ob-feature-icon" />
                <div>
                  <strong>100% On-Device</strong>
                  <span>Runs locally via llama.cpp with zero cloud calls</span>
                </div>
              </div>
              <div className="ob-feature-item">
                <HardDrive size={15} className="ob-feature-icon" />
                <div>
                  <strong>Compact Footprint</strong>
                  <span>Fine-tuned Qwen3-0.6B (~397 MB GGUF, SHA-256 pinned)</span>
                </div>
              </div>
              <div className="ob-feature-item">
                <Lock size={15} className="ob-feature-icon" />
                <div>
                  <strong>Strict Opt-In</strong>
                  <span>Disable anytime in Settings; zero files if declined</span>
                </div>
              </div>
            </div>

            <div className="ob-panel">
              <Checkbox
                label={S.onboardingAccept}
                checked={enabled}
                data-testid="onboarding-ai-checkbox"
                onChange={(_, d) => setEnabled(d.checked === true)}
              />
            </div>

            {downloading ? (
              <div
                role="status"
                data-testid="onboarding-downloading"
                className="ob-downloading"
              >
                <span className="ui-spinner" aria-hidden="true" />
                <span>{S.aiModelDownloading}</span>
              </div>
            ) : (
              <div className="ob-actions">
                <button
                  type="button"
                  className="primary-button ob-primary-cta"
                  onClick={accept}
                  data-testid="onboarding-accept"
                >
                  <span>{enabled ? S.onboardingAccept : S.onboardingDecline}</span>
                  <ArrowRight size={14} />
                </button>
              </div>
            )}

            <p className="ob-footnote">
              <ShieldCheck size={14} />
              <span>{S.onboardingPrivacy}</span>
            </p>
          </>
        )}

        {step === "look" && (
          <>
            <div className="ob-hero-row">
              <div className="ob-logo" aria-hidden="true">
                {choice === "light" ? (
                  <Sun size={22} />
                ) : choice === "dark" ? (
                  <Moon size={22} />
                ) : (
                  <Monitor size={22} />
                )}
              </div>
              <div>
                <div className="ob-eyebrow">WORKSPACE APPEARANCE</div>
                <h2 className="ob-title">{S.onboardingLookTitle}</h2>
              </div>
            </div>

            <p className="ob-lead">{S.onboardingLookBody}</p>

            <div className="ob-themes">
              {(
                [
                  {
                    c: "dark" as ThemeChoice,
                    label: S.themeDark,
                    sub: "Deep carbon surfaces with crisp contrast",
                    rail: "#0a0b0e",
                    list: "#0d0f14",
                    reader: "#090a0d",
                    card: "#181b24",
                    accent: "#2563eb",
                  },
                  {
                    c: "light" as ThemeChoice,
                    label: S.themeLight,
                    sub: "High-key daylight palette for bright rooms",
                    rail: "#f4f5f8",
                    list: "#ffffff",
                    reader: "#fafbfc",
                    card: "#e9eef8",
                    accent: "#1d4ed8",
                  },
                  {
                    c: "system" as ThemeChoice,
                    label: S.themeSystem,
                    sub: "Follows your Windows theme automatically",
                    rail: "linear-gradient(135deg, #0a0b0e 50%, #f4f5f8 50%)",
                    list: "#14171f",
                    reader: "#ffffff",
                    card: "#2563eb",
                    accent: "#3b82f6",
                  },
                ]
              ).map((t) => (
                <button
                  type="button"
                  key={t.c}
                  className={`ob-theme${choice === t.c ? " picked" : ""}`}
                  onClick={() => setChoice(t.c)}
                  data-testid={`onboarding-theme-${t.c}`}
                  aria-pressed={choice === t.c}
                >
                  <span
                    className="ob-theme-prev"
                    style={{ background: t.rail }}
                    aria-hidden="true"
                  >
                    <span
                      className="ob-theme-prev__list"
                      style={{ background: t.list }}
                    >
                      <i style={{ background: t.card }} />
                      <i style={{ background: t.card, opacity: 0.6 }} />
                    </span>
                    <span
                      className="ob-theme-prev__reader"
                      style={{ background: t.reader }}
                    >
                      <b style={{ background: t.accent }} />
                    </span>
                  </span>
                  <span className="ob-theme-nm">{t.label}</span>
                  <span className="ob-theme-sub">{t.sub}</span>
                </button>
              ))}
            </div>

            <div className="ob-actions ob-actions--split">
              <button
                type="button"
                className="link-button"
                onClick={() => setStep("ai")}
              >
                <ArrowLeft size={14} />
                <span>Back</span>
              </button>
              <button
                type="button"
                className="primary-button ob-primary-cta"
                onClick={() => setStep("account")}
              >
                <span>{S.onboardingNext}</span>
                <ArrowRight size={14} />
              </button>
            </div>
          </>
        )}

        {step === "account" && (
          <>
            <div className="ob-hero-row">
              <div className="ob-logo" aria-hidden="true">
                <Mail size={22} />
              </div>
              <div>
                <div className="ob-eyebrow">DIRECT IMAP / SMTP</div>
                <h2 className="ob-title">{S.addAccountTitle}</h2>
              </div>
            </div>

            <p className="ob-lead">{S.onboardingAccountBody}</p>

            <div className="ob-provider-badges" aria-label="Supported providers">
              <span className="ob-provider-chip">Gmail</span>
              <span className="ob-provider-chip">iCloud Mail</span>
              <span className="ob-provider-chip">Zoho Mail</span>
              <span className="ob-provider-chip">Yahoo Mail</span>
              <span className="ob-provider-chip">Custom IMAP</span>
            </div>

            <div className="ob-security-box">
              <ShieldCheck size={16} className="ob-security-box__icon" />
              <div className="ob-security-box__text">
                <strong>Protected by Windows Credential Manager &amp; DPAPI</strong>
                <span>
                  App passwords never touch plaintext config files. Your local
                  mail cache is encrypted at rest with SQLCipher.
                </span>
              </div>
            </div>

            <div className="ob-actions ob-actions--split">
              <button
                type="button"
                className="link-button"
                onClick={() => setStep("look")}
              >
                <ArrowLeft size={14} />
                <span>Back</span>
              </button>
              <div className="ob-actions-right">
                <button
                  type="button"
                  className="link-button"
                  onClick={onFinish}
                  data-testid="onboarding-finish"
                >
                  {S.onboardingSkip}
                </button>
                <button
                  type="button"
                  className="primary-button ob-primary-cta"
                  onClick={onAddAccount}
                  data-testid="onboarding-add-account"
                >
                  <Zap size={14} />
                  <span>{S.addAccount}</span>
                </button>
              </div>
            </div>

            <p className="ob-footnote">
              <Sparkles size={14} />
              <span>{S.onboardingDoneHint}</span>
            </p>
          </>
        )}
      </div>
    </div>
  );
}
