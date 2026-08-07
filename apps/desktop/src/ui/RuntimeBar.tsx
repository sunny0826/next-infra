export function RuntimeBar() {
  return (
    <footer aria-label="控制平面运行时" className="shell-runtime-bar">
      <div className="shell-runtime-cluster">
        <span className="shell-runtime-core shell-status shell-status-unknown">
          <span className="shell-status-dot" /> 运行时未连接
        </span>
        <span>本地 · 只读</span>
      </div>
      <div className="shell-runtime-cluster shell-runtime-secondary">
        <span>Goal 3 查询界面</span>
        <span>已禁用提供方写入</span>
      </div>
    </footer>
  );
}
