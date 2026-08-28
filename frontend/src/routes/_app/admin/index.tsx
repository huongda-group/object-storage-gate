// Ported from console-object-storage-gate/project/Admin.dc.html.
import { Link, createFileRoute, redirect } from "@tanstack/react-router";
import { Header } from "../../../components/Header";
import { useShell } from "../../../components/shell";
import { H1, Page, Panel, monoStyle } from "../../../components/ui";
import { type AdminUser, listUsers } from "../../../lib/admin";
import { colorFor, fmt } from "../../../lib/format";

export const Route = createFileRoute("/_app/admin/")({
  // UX guard only — AdminCaller is the real gate, on the server.
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  loader: () => listUsers(),
  component: AdminDashboard,
});

/// Counts the gateway can compute from the accounts alone.
/// The prototype showed bucket and object totals here; those need an admin-wide aggregate the
/// API does not expose, so they are left out rather than invented.
function stats(users: AdminUser[]) {
  const admins = users.filter((u) => u.role === "admin").length;
  const pending = users.filter((u) => u.must_change_password).length;
  const stored = users.reduce((a, u) => a + u.used_bytes, 0);
  return [
    {
      label: "USER",
      value: String(users.length),
      sub: `${admins} admin · ${pending} chưa đổi mật khẩu tạm`,
      color: "var(--tx)",
    },
    {
      label: "DUNG LƯỢNG",
      value: fmt(stored),
      sub: "tổng trên mọi tài khoản",
      color: "var(--tx)",
    },
    {
      label: "QUOTA KHÔNG GIỚI HẠN",
      value: String(users.filter((u) => u.max_bytes === 0).length),
      sub: "tài khoản",
      color: "var(--tx)",
    },
  ];
}

function AdminDashboard() {
  const { user, requestLogout } = useShell();
  const users: AdminUser[] = Route.useLoaderData();

  const byUsage = [...users].sort((a, b) => b.used_bytes - a.used_bytes);
  const topUsers = byUsage.slice(0, 5);
  const nearlyFull = users.filter(
    (u) => u.max_bytes > 0 && u.used_bytes / u.max_bytes >= 0.9,
  );
  const ADMIN_STATS = stats(users);

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ fontSize: 13, color: "var(--dim)" }}>Admin</span>
            <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
            <span style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
              Hệ thống
            </span>
          </div>
        }
      />
      <Page>
        <H1>Hệ thống</H1>
        <div
          style={{
            fontSize: 13,
            color: "var(--dim)",
            marginTop: 4,
            marginBottom: 18,
          }}
        >
          Toàn bộ gateway · số liệu là bản chụp
        </div>

        <div
          data-grid="astats"
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3,1fr)",
            gap: 14,
          }}
        >
          {ADMIN_STATS.map((s) => (
            <div
              key={s.label}
              style={{
                background: "var(--panel)",
                border: "1px solid var(--line)",
                borderRadius: 12,
                padding: "16px 18px",
              }}
            >
              <div
                style={{
                  fontSize: 11,
                  letterSpacing: ".1em",
                  color: "var(--faint)",
                  fontWeight: 600,
                }}
              >
                {s.label}
              </div>
              <div
                style={{
                  fontSize: 24,
                  fontWeight: 600,
                  letterSpacing: "-.02em",
                  marginTop: 10,
                  ...monoStyle,
                  color: s.color,
                }}
              >
                {s.value}
              </div>
              <div style={{ fontSize: 12, color: "var(--dim)", marginTop: 6 }}>
                {s.sub}
              </div>
            </div>
          ))}
        </div>

        <div
          data-grid="two"
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: 14,
            marginTop: 14,
          }}
        >
          <Panel>
            <div
              style={{
                padding: "14px 18px",
                borderBottom: "1px solid var(--line)",
                fontSize: 13,
                fontWeight: 600,
              }}
            >
              Top user theo dung lượng
            </div>
            <div style={{ padding: "6px 8px" }}>
              {topUsers.map((u) => {
                const pct = u.max_bytes
                  ? Math.min(100, (u.used_bytes / u.max_bytes) * 100)
                  : 0;
                return (
                  <Link
                    key={u.email}
                    to="/admin/users/$pid"
                    params={{ pid: u.pid }}
                    className="rowHover linkPlain"
                    style={{
                      display: "grid",
                      gridTemplateColumns: "1fr 130px 90px",
                      alignItems: "center",
                      gap: 14,
                      width: "100%",
                      padding: 10,
                      borderRadius: 8,
                      textAlign: "left",
                    }}
                  >
                    <div
                      style={{
                        fontSize: 13,
                        color: "var(--tx)",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {u.email}
                    </div>
                    {u.max_bytes ? (
                      <div
                        style={{
                          height: 5,
                          borderRadius: 3,
                          background: "var(--line)",
                          overflow: "hidden",
                          display: "flex",
                        }}
                      >
                        <div
                          style={{
                            width: `${pct.toFixed(1)}%`,
                            background: colorFor(pct),
                          }}
                        />
                      </div>
                    ) : (
                      <div
                        style={{
                          fontSize: 11,
                          color: "var(--acc)",
                          ...monoStyle,
                        }}
                      >
                        ∞ Không giới hạn
                      </div>
                    )}
                    <div
                      style={{
                        fontSize: 12.5,
                        color: "var(--dim)",
                        textAlign: "right",
                        ...monoStyle,
                      }}
                    >
                      {fmt(u.used_bytes)}
                    </div>
                  </Link>
                );
              })}
            </div>
          </Panel>

          <Panel>
            <div
              style={{
                padding: "14px 18px",
                borderBottom: "1px solid var(--line)",
                fontSize: 13,
                fontWeight: 600,
                display: "flex",
                alignItems: "center",
                gap: 8,
              }}
            >
              User sắp đầy quota{" "}
              <span
                style={{
                  fontSize: 11,
                  fontWeight: 600,
                  padding: "2px 7px",
                  borderRadius: 20,
                  background: "var(--dgrSoft)",
                  color: "var(--dgr)",
                }}
              >
                ≥90%
              </span>
            </div>
            <div style={{ padding: "6px 8px" }}>
              {nearlyFull.map((u) => {
                const pct = Math.min(100, (u.used_bytes / u.max_bytes) * 100);
                return (
                  <Link
                    key={u.email}
                    to="/admin/users/$pid"
                    params={{ pid: u.pid }}
                    className="rowHover linkPlain"
                    style={{
                      display: "grid",
                      gridTemplateColumns: "1fr 130px 60px",
                      alignItems: "center",
                      gap: 14,
                      width: "100%",
                      padding: 10,
                      borderRadius: 8,
                      textAlign: "left",
                    }}
                  >
                    <div
                      style={{
                        fontSize: 13,
                        color: "var(--tx)",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {u.email}
                    </div>
                    <div
                      style={{
                        height: 5,
                        borderRadius: 3,
                        background: "var(--line)",
                        overflow: "hidden",
                        display: "flex",
                      }}
                    >
                      <div
                        style={{
                          width: `${pct.toFixed(1)}%`,
                          background: colorFor(pct),
                        }}
                      />
                    </div>
                    <div
                      style={{
                        fontSize: 12.5,
                        color: colorFor(pct),
                        textAlign: "right",
                        ...monoStyle,
                        fontWeight: 600,
                      }}
                    >
                      {pct.toFixed(0)}%
                    </div>
                  </Link>
                );
              })}
            </div>
          </Panel>
        </div>
      </Page>
    </>
  );
}
