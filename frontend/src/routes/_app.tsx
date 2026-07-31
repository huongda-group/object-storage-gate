import {
  Outlet,
  createFileRoute,
  redirect,
  useNavigate,
  useRouterState,
} from "@tanstack/react-router";
import { useState } from "react";
import {
  CancelButton,
  Modal,
  ModalActions,
  ModalText,
  ModalTitle,
} from "../components/Modal";
import { type NavKey, Sidebar } from "../components/Sidebar";
import { ToastProvider } from "../components/Toast";
import { ShellProvider } from "../components/shell";
import { clearToken, currentCached, getToken, logout } from "../lib/auth";

export const Route = createFileRoute("/_app")({
  // UX guard only — real enforcement is server-side (slice #7).
  beforeLoad: async () => {
    if (!getToken()) throw redirect({ to: "/login" });
    try {
      return { user: await currentCached() };
    } catch {
      clearToken();
      throw redirect({ to: "/login" });
    }
  },
  component: AppShell,
});

function navKeyFor(pathname: string): NavKey {
  if (pathname === "/") return "dash";
  if (pathname.startsWith("/buckets")) return "buckets";
  if (pathname.startsWith("/keys")) return "keys";
  if (pathname.startsWith("/api")) return "api";
  if (pathname.startsWith("/admin/users")) return "users";
  if (pathname.startsWith("/admin/buckets")) return "abuckets";
  if (pathname.startsWith("/admin")) return "admin";
  return "none";
}

function AppShell() {
  const { user } = Route.useRouteContext();
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const [confirm, setConfirm] = useState(false);

  return (
    <ToastProvider>
      <div
        style={{
          display: "flex",
          minHeight: "100vh",
          background: "var(--bg)",
        }}
      >
        <Sidebar
          active={navKeyFor(pathname)}
          isAdmin={user.role === "admin"}
          onLogout={() => setConfirm(true)}
        />
        <main
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            flexDirection: "column",
          }}
        >
          <ShellProvider
            value={{ user, requestLogout: () => setConfirm(true) }}
          >
            <Outlet />
          </ShellProvider>
        </main>
      </div>

      {confirm && (
        <Modal onClose={() => setConfirm(false)}>
          <ModalTitle>Đăng xuất?</ModalTitle>
          <ModalText>
            Bạn sẽ cần đăng nhập lại để tiếp tục sử dụng console.
          </ModalText>
          <ModalActions>
            <CancelButton onClick={() => setConfirm(false)} />
            <button
              type="button"
              onClick={() => {
                logout();
                navigate({ to: "/login" });
              }}
              style={{
                height: 34,
                padding: "0 14px",
                border: 0,
                borderRadius: 8,
                background: "var(--dgr)",
                color: "#1A0D03",
                fontSize: 13,
                fontWeight: 600,
                cursor: "pointer",
              }}
            >
              Đăng xuất
            </button>
          </ModalActions>
        </Modal>
      )}
    </ToastProvider>
  );
}
