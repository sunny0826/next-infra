import { createContext, useContext, type ReactNode } from "react";

import type { DesktopAdapter } from "./desktop-adapter";

const DesktopAdapterContext = createContext<DesktopAdapter | null>(null);

interface DesktopAdapterProviderProps {
  readonly adapter: DesktopAdapter;
  readonly children: ReactNode;
}

export function DesktopAdapterProvider({
  adapter,
  children,
}: DesktopAdapterProviderProps) {
  return (
    <DesktopAdapterContext.Provider value={adapter}>
      {children}
    </DesktopAdapterContext.Provider>
  );
}

export function useDesktopAdapter(): DesktopAdapter {
  const adapter = useContext(DesktopAdapterContext);

  if (adapter === null) {
    throw new Error(
      "useDesktopAdapter must be used within a DesktopAdapterProvider",
    );
  }

  return adapter;
}
