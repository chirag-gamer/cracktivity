/** Add-account dialog (FR-1): preset pick → app password → immediate sync. */
import { useState } from "react";
import {
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Dropdown,
  Field,
  Input,
  Option,
  Spinner,
  Text,
} from "@fluentui/react-components";
import { ExternalLink, KeyRound, Mail, Server, ShieldCheck } from "lucide-react";
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";

/** Mirrors src-tauri/src/platform/providers.rs (verified presets, D-021). */
const PROVIDERS = [
  {
    id: "gmail",
    label: "Gmail",
    server: "imap.gmail.com:993 · smtp.gmail.com:465",
    note: "Requires 2-Step Verification; create a 16-character app password",
    url: "https://myaccount.google.com/apppasswords",
  },
  {
    id: "icloud",
    label: "iCloud",
    server: "imap.mail.me.com:993 · smtp.mail.me.com:587",
    note: "App-specific password from account.apple.com",
    url: "https://account.apple.com/account/manage",
  },
  {
    id: "zoho",
    label: "Zoho Mail",
    server: "imap.zoho.com:993 · smtp.zoho.com:465",
    note: "Enable IMAP in Zoho settings first",
    url: "https://accounts.zoho.com/home#security/appPasswords",
  },
  {
    id: "yahoo",
    label: "Yahoo Mail",
    server: "imap.mail.yahoo.com:993 · smtp.mail.yahoo.com:465",
    note: "Account Security → External connections",
    url: "https://login.yahoo.com/account/security",
  },
  {
    id: "generic",
    label: "Other (IMAP)",
    server: "Custom TLS IMAP (993) & SMTP (465/587)",
    note: "Enter the server settings your provider documents",
    url: null,
  },
] as const;

export function AddAccountDialog({
  open,
  onClose,
  onAdded,
}: {
  open: boolean;
  onClose: () => void;
  onAdded: () => void;
}) {
  const [provider, setProvider] = useState<string>("gmail");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [host, setHost] = useState("");
  const [smtpHost, setSmtpHost] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const preset = PROVIDERS.find((p) => p.id === provider) ?? PROVIDERS[0];

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const trimmedImap = host.trim();
      const trimmedSmtp =
        smtpHost.trim() ||
        (trimmedImap ? trimmedImap.replace(/^imap\./i, "smtp.") : "");
      await ipc.addAccount(
        provider,
        email.trim(),
        password,
        provider === "generic" ? trimmedImap : undefined,
        undefined,
        provider === "generic" ? trimmedSmtp : undefined,
      );
      onAdded();
      onClose();
      setEmail("");
      setPassword("");
      setHost("");
      setSmtpHost("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface className="add-account-surface">
        <DialogBody>
          <DialogTitle>
            <span className="dialog-title-row">
              <span className="dialog-title-icon">
                <Mail size={16} />
              </span>
              <span>{S.addAccountTitle}</span>
            </span>
          </DialogTitle>
          <DialogContent className="add-account-body">
            <div className="provider-pill-strip" role="group" aria-label="Quick select provider">
              {PROVIDERS.map((p) => (
                <button
                  type="button"
                  key={p.id}
                  className={`provider-pill-btn${provider === p.id ? " is-active" : ""}`}
                  aria-pressed={provider === p.id}
                  onClick={() => setProvider(p.id)}
                >
                  {p.label}
                </button>
              ))}
            </div>

            <Field label={S.provider}>
              <Dropdown
                value={preset.label}
                selectedOptions={[provider]}
                onOptionSelect={(_, d) => setProvider(String(d.optionValue))}
                data-testid="provider-dropdown"
              >
                {PROVIDERS.map((p) => (
                  <Option key={p.id} value={p.id} text={p.label}>
                    {p.label}
                  </Option>
                ))}
              </Dropdown>
            </Field>

            <div className="provider-guide-card">
              <div className="provider-guide-server">
                <Server size={12} />
                <code>{preset.server}</code>
              </div>
              <Text size={200} className="provider-guide-note">
                <KeyRound size={12} style={{ display: "inline", verticalAlign: "-2px", marginRight: 5 }} />
                {preset.note}
                {preset.url && (
                  <>
                    {" "}
                    <a
                      href={preset.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="provider-guide-link"
                    >
                      <span>{S.providerPage}</span>
                      <ExternalLink size={11} />
                    </a>
                  </>
                )}
              </Text>
            </div>

            <Field label={S.email}>
              <Input
                type="email"
                placeholder="you@example.com"
                value={email}
                onChange={(_, d) => setEmail(d.value)}
                data-testid="account-email"
              />
            </Field>

            <Field label={S.appPassword}>
              <Input
                type="password"
                placeholder="•••• •••• •••• ••••"
                value={password}
                onChange={(_, d) => setPassword(d.value)}
                data-testid="account-password"
              />
            </Field>

            {provider === "generic" && (
              <>
                <Field label={S.imapHost}>
                  <Input
                    value={host}
                    placeholder="imap.example.com"
                    onChange={(_, d) => setHost(d.value)}
                    data-testid="account-host"
                  />
                </Field>
                <Field label="SMTP server host">
                  <Input
                    value={smtpHost}
                    placeholder={
                      host.trim()
                        ? host.trim().replace(/^imap\./i, "smtp.")
                        : "smtp.example.com"
                    }
                    onChange={(_, d) => setSmtpHost(d.value)}
                    data-testid="account-smtp-host"
                  />
                </Field>
              </>
            )}

            {error && (
              <div className="dialog-error-banner" role="alert">
                <Text size={200} className="error-text">
                  {error}
                </Text>
              </div>
            )}

            <div className="dialog-security-foot">
              <ShieldCheck size={13} />
              <span>Stored in Windows Credential Manager (never plaintext on disk)</span>
            </div>
          </DialogContent>
          <DialogActions>
            {busy && <Spinner size="tiny" />}
            <button type="button" className="link-button" onClick={onClose}>
              {S.cancel}
            </button>
            <button
              type="button"
              className="primary-button"
              onClick={() => void submit()}
              disabled={busy || !email.includes("@") || password.length === 0}
              data-testid="add-account-submit"
            >
              {S.add}
            </button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
