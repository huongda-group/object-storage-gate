import type React from "react";
import { createContext, useContext } from "react";
import type { CurrentUser } from "../lib/auth";

type Shell = { user: CurrentUser; requestLogout: () => void };

const ShellContext = createContext<Shell | null>(null);

export function ShellProvider({
  value,
  children,
}: {
  value: Shell;
  children: React.ReactNode;
}) {
  return (
    <ShellContext.Provider value={value}>{children}</ShellContext.Provider>
  );
}

/** Current user + logout prompt, for the screens rendered inside `_app`. */
export function useShell(): Shell {
  const shell = useContext(ShellContext);
  if (!shell) throw new Error("useShell used outside the app shell");
  return shell;
}
