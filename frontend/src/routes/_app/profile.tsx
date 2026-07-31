// Ported from console-object-storage-gate/project/Profile.dc.html.
import { Link, createFileRoute } from "@tanstack/react-router";
import { Header, initialsOf } from "../../components/Header";
import { useShell } from "../../components/shell";
import { Page, monoStyle } from "../../components/ui";
import { fmt } from "../../lib/format";
import { ACCOUNT, BUCKETS, KEYS } from "../../lib/mock";

export const Route = createFileRoute("/_app/profile")({ component: Profile });

const statCard = {
  background: "var(--panel)",
  border: "1px solid var(--line)",
  borderRadius: 12,
  padding: 16,
} as const;

const statLabel = {
  fontSize: 11,
  letterSpacing: ".1em",
  color: "var(--faint)",
  fontWeight: 600,
} as const;

const statValue = {
  fontSize: 22,
  fontWeight: 600,
  letterSpacing: "-.02em",
  marginTop: 8,
  fontFamily: "'IBM Plex Mono',monospace",
} as const;

function Profile() {
  const { user, requestLogout } = useShell();
  const activeKeys = KEYS.filter((k) => k.status === "active").length;

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            Hồ sơ
          </span>
        }
      />
      <Page maxWidth={1000}>
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: 20,
            marginBottom: 24,
            flexWrap: "wrap",
          }}
        >
          <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
            <div
              style={{
                width: 56,
                height: 56,
                flex: "0 0 56px",
                borderRadius: "50%",
                background: "var(--accSoft)",
                border: "1px solid var(--accLine)",
                color: "var(--acc)",
                display: "grid",
                placeItems: "center",
                fontSize: 19,
                fontWeight: 600,
              }}
            >
              {initialsOf(user.name)}
            </div>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <h1
                  style={{
                    fontSize: 20,
                    fontWeight: 600,
                    margin: 0,
                    letterSpacing: "-.01em",
                  }}
                >
                  {user.name}
                </h1>
                <span
                  style={{
                    fontSize: 11,
                    fontWeight: 600,
                    padding: "3px 8px",
                    borderRadius: 5,
                    background:
                      user.role === "admin"
                        ? "var(--infoSoft)"
                        : "var(--panel2)",
                    color: user.role === "admin" ? "var(--info)" : "var(--dim)",
                  }}
                >
                  {user.role.toUpperCase()}
                </span>
              </div>
              <div
                style={{
                  fontSize: 13,
                  color: "var(--dim)",
                  marginTop: 6,
                  ...monoStyle,
                }}
              >
                {user.email}
              </div>
              <div
                style={{ fontSize: 12.5, color: "var(--faint)", marginTop: 4 }}
              >
                Quota tài khoản:{" "}
                {user.max_bytes ? fmt(user.max_bytes) : "Không giới hạn"}
              </div>
            </div>
          </div>
          <Link
            to="/settings"
            className="btnGhost"
            style={{
              height: 34,
              padding: "0 16px",
              border: "1px solid var(--line2)",
              background: "var(--panel)",
              color: "var(--tx)",
              borderRadius: 8,
              fontSize: 13,
              fontWeight: 500,
              display: "inline-flex",
              alignItems: "center",
            }}
          >
            Sửa hồ sơ &amp; mật khẩu
          </Link>
        </div>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3,1fr)",
            gap: 12,
            marginBottom: 14,
          }}
        >
          <div style={statCard}>
            <div style={statLabel}>DUNG LƯỢNG ĐÃ DÙNG</div>
            <div style={statValue}>{fmt(ACCOUNT.used)}</div>
          </div>
          <div style={statCard}>
            <div style={statLabel}>BUCKET SỞ HỮU</div>
            <div style={statValue}>{BUCKETS.length}</div>
          </div>
          <div style={statCard}>
            <div style={statLabel}>ACCESS KEY HOẠT ĐỘNG</div>
            <div style={statValue}>{activeKeys}</div>
          </div>
        </div>
      </Page>
    </>
  );
}
