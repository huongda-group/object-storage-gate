import { Outlet, createFileRoute, redirect } from "@tanstack/react-router";
import { setupStatus } from "../lib/auth";

export const Route = createFileRoute("/_auth")({
  // ponytail: one GET per auth-page load, no cache — setup can complete in
  // another tab, so a stale "needs setup" would strand the visitor on /setup.
  beforeLoad: async ({ location }) => {
    const onSetup = location.pathname === "/setup";
    const { needs_setup } = await setupStatus().catch(() => ({
      needs_setup: false,
    }));
    if (needs_setup && !onSetup) throw redirect({ to: "/setup" });
    if (!needs_setup && onSetup) throw redirect({ to: "/login" });
  },
  component: AuthShell,
});

function AuthShell() {
  return (
    <div
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        background: "var(--bg)",
        padding: "32px 20px",
      }}
    >
      <div style={{ width: "100%", maxWidth: 360 }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 9,
            marginBottom: 26,
          }}
        >
          <div
            style={{
              width: 24,
              height: 24,
              borderRadius: 7,
              background: "var(--acc)",
              display: "grid",
              placeItems: "center",
            }}
          >
            <div
              style={{
                width: 9,
                height: 9,
                borderRadius: 3,
                background: "var(--accTx)",
              }}
            />
          </div>
          <div
            style={{ fontWeight: 600, fontSize: 14, letterSpacing: "-.01em" }}
          >
            Object Storage Gate
          </div>
        </div>
        <Outlet />
      </div>
    </div>
  );
}
