import { Outlet, createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/_auth")({ component: AuthShell });

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
