import { SHELL_ROUTES, type RouteId } from "../app/routes";
import { Icon } from "./Icon";

interface NavigationProps {
  activeRoute: RouteId;
  onNavigate: (route: RouteId) => void;
}

export function Navigation({ activeRoute, onNavigate }: NavigationProps) {
  return (
    <aside className="shell-navigation">
      <div className="shell-brand">
        <span className="shell-brand-mark">
          <Icon name="topology" />
        </span>
        <span className="shell-brand-name">Next Infra</span>
      </div>

      <nav aria-label="Primary navigation" className="shell-nav-list">
        {SHELL_ROUTES.map((route) => (
          <button
            aria-current={activeRoute === route.id ? "page" : undefined}
            aria-label={route.label}
            className="shell-nav-item"
            data-shell-route={route.id}
            key={route.id}
            onClick={() => onNavigate(route.id)}
            title={route.label}
            type="button"
          >
            <Icon name={route.icon} />
            <span className="shell-nav-label">{route.label}</span>
          </button>
        ))}
      </nav>

      <div className="shell-navigation-foot">
        <strong>本地工作区</strong>
        <span>单用户 · 此 Mac</span>
      </div>
    </aside>
  );
}
