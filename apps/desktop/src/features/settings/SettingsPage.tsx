import { useEffect, useState } from "react";

import type { LocalSettings, RuntimeCapabilities } from "../../platform/desktop-adapter/desktop-adapter";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./settings.css";

interface SettingsState { readonly settings: LocalSettings; readonly capabilities: RuntimeCapabilities; }

export function SettingsPage() {
  const adapter = useDesktopAdapter();
  const [state, setState] = useState<SettingsState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([adapter.getLocalSettings(), adapter.getRuntimeCapabilities()])
      .then(([settings, capabilities]) => { if (active) setState({ settings, capabilities }); })
      .catch(() => { if (active) setError("Local settings could not be queried."); });
    return () => { active = false; };
  }, [adapter]);

  async function save(settings: LocalSettings) {
    if (state === null) return;
    try {
      const saved = await adapter.updateLocalSettings(settings);
      setState({ ...state, settings: saved });
    } catch {
      setError("Local settings could not be updated.");
    }
  }

  if (error !== null) return <section className="settings-state settings-state--error" role="alert">{error}</section>;
  if (state === null) return <section className="settings-state" aria-busy="true">Reading local settings…</section>;

  const { settings, capabilities } = state;
  return (
    <div className="settings-page">
      <header><p className="settings-eyebrow">Local control plane</p><h1>Settings</h1><p>Configure this Mac without changing provider resources.</p></header>
      <section aria-labelledby="settings-lifecycle"><h2 id="settings-lifecycle">Lifecycle</h2>
        <div className="settings-row"><div><strong>Start at login</strong><p>Launch the signed Desktop Host for this user session.</p></div><button aria-pressed={settings.start_at_login} className="settings-switch" disabled={!capabilities.start_at_login} onClick={() => save({ ...settings, start_at_login: !settings.start_at_login })} type="button"><span />{settings.start_at_login ? "On" : "Off"}</button></div>
        <div className="settings-row"><div><strong>MCP auto-launch</strong><p>Separate trusted Host availability capability; it is not start-at-login.</p></div><button aria-pressed="false" className="settings-switch" disabled={!capabilities.mcp_auto_launch} type="button"><span />Unavailable</button></div>
        <div className="settings-row"><div><strong>User quit latch</strong><p>Visible suppression state. Agents cannot clear an explicit user quit.</p></div><output>{settings.user_quit ? "latched" : "clear"}</output></div>
      </section>
      <section aria-labelledby="settings-storage"><h2 id="settings-storage">Local data</h2>
        <label className="settings-row"><div><strong>Data budget</strong><p>Maximum local projection budget in megabytes.</p></div><input min="64" onChange={(event) => save({ ...settings, data_budget_mb: Number(event.currentTarget.value) })} type="number" value={settings.data_budget_mb} /></label>
        <label className="settings-row"><div><strong>Retention</strong><p>Days to retain bounded local history.</p></div><input min="1" onChange={(event) => save({ ...settings, retention_days: Number(event.currentTarget.value) })} type="number" value={settings.retention_days} /></label>
      </section>
      <section aria-labelledby="settings-boundary"><h2 id="settings-boundary">Security boundary</h2><div className="settings-notice">Credentials remain in macOS Keychain. Existing Secret values and SecretRef identifiers are not rendered by this page.</div></section>
    </div>
  );
}
