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
import { ipc } from "../shared/ipc";
import { S } from "../shared/strings";

/** Mirrors src-tauri/src/platform/providers.rs (verified presets, D-021). */
const PROVIDERS = [
  { id: "gmail", label: "Gmail", note: "Requires 2-Step Verification; create a 16-character app password", url: "https://myaccount.google.com/apppasswords" },
  { id: "icloud", label: "iCloud", note: "App-specific password from account.apple.com", url: "https://account.apple.com/account/manage" },
  { id: "zoho", label: "Zoho Mail", note: "Enable IMAP in Zoho settings first", url: "https://accounts.zoho.com/home#security/appPasswords" },
  { id: "yahoo", label: "Yahoo Mail", note: "Account Security → External connections", url: "https://login.yahoo.com/account/security" },
  { id: "generic", label: "Other (IMAP)", note: "Enter the server settings your provider documents", url: null },
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const preset = PROVIDERS.find((p) => p.id === provider)!;

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await ipc.addAccount(
        provider,
        email.trim(),
        password,
        provider === "generic" ? host.trim() : undefined,
      );
      onAdded();
      onClose();
      setEmail("");
      setPassword("");
      setHost("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{S.addAccountTitle}</DialogTitle>
          <DialogContent className="add-account-body">
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
            <Text size={200}>
              {preset.note}
              {preset.url && (
                <>
                  {" "}
                  <a href={preset.url} target="_blank" rel="noopener noreferrer">
                    open provider page
                  </a>
                </>
              )}
            </Text>
            <Field label={S.email}>
              <Input
                type="email"
                value={email}
                onChange={(_, d) => setEmail(d.value)}
                data-testid="account-email"
              />
            </Field>
            <Field label={S.appPassword}>
              <Input
                type="password"
                value={password}
                onChange={(_, d) => setPassword(d.value)}
                data-testid="account-password"
              />
            </Field>
            {provider === "generic" && (
              <Field label={S.imapHost}>
                <Input
                  value={host}
                  placeholder="imap.example.com"
                  onChange={(_, d) => setHost(d.value)}
                  data-testid="account-host"
                />
              </Field>
            )}
            {error && (
              <Text size={200} className="error-text">
                {error}
              </Text>
            )}
          </DialogContent>
          <DialogActions>
            {busy && <Spinner size="tiny" />}
            <button className="link-button" onClick={onClose}>
              {S.cancel}
            </button>
            <button
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
