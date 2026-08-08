import { useEffect, useState } from "react";

import type { LocalSettings, RuntimeCapabilities } from "../../platform/desktop-adapter/desktop-adapter";
import { displayRuntimeReason } from "../../i18n";
import { useTheme } from "../../hooks/useTheme";
import { useDesktopAdapter } from "../../platform/desktop-adapter/DesktopAdapterContext";

import "./settings.css";

interface SettingsState { readonly settings: LocalSettings; readonly capabilities: RuntimeCapabilities; }

export function SettingsPage() {
  const adapter = useDesktopAdapter();
  const { theme, toggleTheme } = useTheme();
  const [state, setState] = useState<SettingsState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([adapter.getLocalSettings(), adapter.getRuntimeCapabilities()])
      .then(([settings, capabilities]) => { if (active) setState({ settings, capabilities }); })
      .catch(() => { if (active) setError("无法查询本地设置。"); });
    return () => { active = false; };
  }, [adapter]);

  async function save(settings: LocalSettings) {
    if (state === null) return;
    try {
      const saved = await adapter.updateLocalSettings(settings);
      setState({ ...state, settings: saved });
    } catch {
      setError("无法更新本地设置。");
    }
  }

  if (error !== null) return <section className="settings-state settings-state--error" role="alert">{error}</section>;
  if (state === null) return <section className="settings-state" aria-busy="true">正在读取本地设置…</section>;

  const { settings, capabilities } = state;
  return (
    <div className="settings-page">
      <header><p className="settings-eyebrow">本地控制平面</p><h1>设置</h1><p>配置此 Mac，不改变提供方资源。</p></header>
      <section aria-labelledby="settings-lifecycle"><h2 id="settings-lifecycle">生命周期</h2>
        <div className="settings-row"><div><strong>登录时启动</strong><p>为当前用户会话启动已签名的 Desktop Host。</p></div><button aria-pressed={settings.start_at_login} className="settings-switch" disabled={!capabilities.start_at_login} onClick={() => save({ ...settings, start_at_login: !settings.start_at_login })} type="button"><span />{settings.start_at_login ? "开启" : "关闭"}</button></div>
        <div className="settings-row"><div><strong>MCP 自动启动</strong><p>独立的受信 Host 可用性能力，不等于登录时启动。</p><p className="settings-guidance">{displayRuntimeReason(capabilities.mcp_auto_launch_reason)}</p></div><output className={capabilities.mcp_auto_launch ? "settings-status settings-status--available" : "settings-status"}>{capabilities.mcp_auto_launch ? "可用" : "不可用"}</output></div>
        <div className="settings-row"><div><strong>用户退出锁定</strong><p>Agent 和 MCP 请求不能清除用户的明确退出。</p><p className="settings-guidance">{settings.user_quit ? "请交互式重新打开 Next Infra，或等待下一次启用的登录启动以解除抑制。" : "当前没有明确退出抑制。"}</p></div><output className={settings.user_quit ? "settings-status settings-status--suppressed" : "settings-status settings-status--available"}>{settings.user_quit ? "已锁定" : "未锁定"}</output></div>
      </section>
      <section aria-labelledby="settings-storage"><h2 id="settings-storage">本地数据</h2>
        <label className="settings-row"><div><strong>数据预算</strong><p>本地投影预算上限（MB）。</p></div><input min="64" onChange={(event) => save({ ...settings, data_budget_mb: Number(event.currentTarget.value) })} type="number" value={settings.data_budget_mb} /></label>
        <label className="settings-row"><div><strong>保留期限</strong><p>保留受限本地历史的天数。</p></div><input min="1" onChange={(event) => save({ ...settings, retention_days: Number(event.currentTarget.value) })} type="number" value={settings.retention_days} /></label>
      </section>
      <section aria-labelledby="settings-boundary"><h2 id="settings-boundary">安全边界</h2><div className="settings-notice">GitHub MVP 凭据保存在受限本地文件中；本页面不会显示 Secret 值或 SecretRef 标识符。</div></section>
      <section aria-labelledby="settings-appearance"><h2 id="settings-appearance">外观</h2>
        <div className="settings-row"><div><strong>主题</strong><p>在亮色与暗色之间切换显示。</p></div><button aria-pressed={theme === "light"} className="settings-switch" onClick={toggleTheme} type="button"><span />{theme === "light" ? "亮色" : "暗色"}</button></div>
      </section>
    </div>
  );
}
