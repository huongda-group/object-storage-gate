// Ported verbatim from console-object-storage-gate/project/Sidebar.dc.html.
import { Link } from "@tanstack/react-router";
import type React from "react";
import { useEffect, useState } from "react";

export type NavKey =
  | "dash"
  | "buckets"
  | "keys"
  | "api"
  | "admin"
  | "users"
  | "abuckets"
  | "none";

const COLLAPSED_KEY = "osg_collapsed";

const rowStyle = (on: boolean): React.CSSProperties => ({
  display: "flex",
  alignItems: "center",
  gap: 10,
  width: "100%",
  height: 36,
  padding: "0 10px",
  borderRadius: 8,
  textAlign: "left",
  fontSize: 13,
  fontWeight: 500,
  background: on ? "var(--accSoft)" : "transparent",
  color: on ? "var(--acc)" : "var(--dim)",
});

const groupLabel: React.CSSProperties = {
  fontSize: 10,
  letterSpacing: ".14em",
  color: "var(--faint)",
  fontWeight: 600,
  padding: "6px 8px 4px",
};

const footerBtn: React.CSSProperties = {
  flex: "1 1 auto",
  height: 30,
  minHeight: 30,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--dim)",
  borderRadius: 7,
  fontSize: 12,
  display: "grid",
  placeItems: "center",
  cursor: "pointer",
};

function IconDash() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ flex: "0 0 16px" }}
    >
      <rect
        x="1.6"
        y="1.6"
        width="5.3"
        height="5.3"
        rx="1.4"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <rect
        x="9.1"
        y="1.6"
        width="5.3"
        height="5.3"
        rx="1.4"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <rect
        x="1.6"
        y="9.1"
        width="5.3"
        height="5.3"
        rx="1.4"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <rect
        x="9.1"
        y="9.1"
        width="5.3"
        height="5.3"
        rx="1.4"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
    </svg>
  );
}

function IconBucket() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ flex: "0 0 16px" }}
    >
      <rect
        x="2"
        y="2.4"
        width="12"
        height="11.2"
        rx="2.4"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <line
        x1="2"
        y1="6.4"
        x2="14"
        y2="6.4"
        stroke="currentColor"
        strokeWidth="1.4"
      />
    </svg>
  );
}

function IconApi() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ flex: "0 0 16px" }}
    >
      <path
        d="M6 3.2C4.4 3.2 4.6 6.6 3 8c1.6 1.4 1.4 4.8 3 4.8"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      <path
        d="M10 3.2c1.6 0 1.4 3.4 3 4.8-1.6 1.4-1.4 4.8-3 4.8"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

function IconKey() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ flex: "0 0 16px" }}
    >
      <circle
        cx="5.4"
        cy="5.4"
        r="3.4"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <line
        x1="7.9"
        y1="7.9"
        x2="13.6"
        y2="13.6"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <line
        x1="11.2"
        y1="13.6"
        x2="13.6"
        y2="11.2"
        stroke="currentColor"
        strokeWidth="1.4"
      />
    </svg>
  );
}

function IconSystem() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ flex: "0 0 16px" }}
    >
      <rect
        x="2"
        y="9"
        width="3"
        height="5"
        rx="1"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <rect
        x="6.5"
        y="5.5"
        width="3"
        height="8.5"
        rx="1"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <rect
        x="11"
        y="2"
        width="3"
        height="12"
        rx="1"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
    </svg>
  );
}

function IconUsers() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ flex: "0 0 16px" }}
    >
      <circle
        cx="8"
        cy="5.2"
        r="2.8"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <rect
        x="2.6"
        y="9.6"
        width="10.8"
        height="4.6"
        rx="2.3"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
    </svg>
  );
}

function IconPool() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      aria-hidden="true"
      style={{ flex: "0 0 16px" }}
    >
      <rect
        x="2"
        y="2.4"
        width="12"
        height="11.2"
        rx="2.4"
        stroke="currentColor"
        strokeWidth="1.4"
        fill="none"
      />
      <line
        x1="2"
        y1="6.4"
        x2="14"
        y2="6.4"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <line
        x1="5.4"
        y1="9.4"
        x2="10.6"
        y2="9.4"
        stroke="currentColor"
        strokeWidth="1.4"
      />
    </svg>
  );
}

export function Sidebar({
  active,
  isAdmin,
  onLogout,
}: {
  active: NavKey;
  isAdmin: boolean;
  onLogout: () => void;
}) {
  const [collapsed, setCollapsed] = useState(false);

  useEffect(() => {
    if (localStorage.getItem(COLLAPSED_KEY) === "1") setCollapsed(true);
  }, []);

  function toggle() {
    const next = !collapsed;
    setCollapsed(next);
    localStorage.setItem(COLLAPSED_KEY, next ? "1" : "0");
  }

  const width = collapsed ? "64px" : "240px";
  const showLabels = !collapsed;

  return (
    <aside
      style={{
        width,
        flex: `0 0 ${width}`,
        background: "var(--panel)",
        borderRight: "1px solid var(--line)",
        display: "flex",
        flexDirection: "column",
        position: "sticky",
        top: 0,
        height: "100vh",
        overflow: "hidden",
        transition: "width .12s ease",
      }}
    >
      <div
        style={{
          height: 56,
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "0 14px",
          borderBottom: "1px solid var(--line)",
          flex: "0 0 56px",
        }}
      >
        <Link
          to="/"
          style={{
            width: 26,
            height: 26,
            flex: "0 0 26px",
            borderRadius: 8,
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
        </Link>
        {showLabels && (
          <div style={{ minWidth: 0 }}>
            <div
              style={{
                fontSize: 13,
                fontWeight: 600,
                letterSpacing: "-.01em",
                whiteSpace: "nowrap",
              }}
            >
              Object Storage Gate
            </div>
            <div
              style={{
                fontSize: 10,
                color: "var(--faint)",
                fontFamily: "'IBM Plex Mono',monospace",
                whiteSpace: "nowrap",
              }}
            >
              gateway console
            </div>
          </div>
        )}
      </div>

      <nav
        style={{
          flex: 1,
          padding: "14px 10px",
          display: "flex",
          flexDirection: "column",
          gap: 4,
          overflowY: "auto",
        }}
      >
        {showLabels && <div style={groupLabel}>STORAGE</div>}
        <Link to="/" aria-label="Dashboard" style={rowStyle(active === "dash")}>
          <IconDash />
          {showLabels && <span>Dashboard</span>}
        </Link>
        <Link
          to="/buckets"
          aria-label="Buckets"
          style={rowStyle(active === "buckets")}
        >
          <IconBucket />
          {showLabels && <span>Buckets</span>}
        </Link>
        <Link
          to="/keys"
          aria-label="Access Keys"
          style={rowStyle(active === "keys")}
        >
          <IconKey />
          {showLabels && <span>Access Keys</span>}
        </Link>
        <Link to="/api" aria-label="API" style={rowStyle(active === "api")}>
          <IconApi />
          {showLabels && <span>API</span>}
        </Link>

        {isAdmin && (
          <>
            <div style={{ height: 10 }} />
            {showLabels && <div style={groupLabel}>ADMIN</div>}
            <Link
              to="/admin"
              aria-label="Hệ thống"
              style={rowStyle(active === "admin")}
            >
              <IconSystem />
              {showLabels && <span>Hệ thống</span>}
            </Link>
            <Link
              to="/admin/users"
              aria-label="Users"
              style={rowStyle(active === "users")}
            >
              <IconUsers />
              {showLabels && <span>Users</span>}
            </Link>
            <Link
              to="/admin/buckets"
              aria-label="Pool (Admin)"
              style={rowStyle(active === "abuckets")}
            >
              <IconPool />
              {showLabels && <span>Pool</span>}
            </Link>
          </>
        )}
      </nav>

      <div
        style={{
          borderTop: "1px solid var(--line)",
          padding: 10,
          flex: "0 0 auto",
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: collapsed ? "column" : "row",
            gap: 6,
            marginTop: 6,
          }}
        >
          <Link
            to="/settings"
            aria-label="Settings"
            className="btnGhost"
            style={footerBtn}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
              <circle
                cx="8"
                cy="8"
                r="2.4"
                stroke="currentColor"
                strokeWidth="1.4"
                fill="none"
              />
              <circle
                cx="8"
                cy="8"
                r="6"
                stroke="currentColor"
                strokeWidth="1.4"
                fill="none"
                strokeDasharray="2.6 2.2"
              />
            </svg>
          </Link>
          <button
            type="button"
            aria-label="Đăng xuất"
            className="btnDanger"
            onClick={onLogout}
            style={footerBtn}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="M6.4 2.4H3.4a1.4 1.4 0 0 0-1.4 1.4v8.4a1.4 1.4 0 0 0 1.4 1.4h3"
                stroke="currentColor"
                strokeWidth="1.4"
                fill="none"
              />
              <line
                x1="7"
                y1="8"
                x2="13.6"
                y2="8"
                stroke="currentColor"
                strokeWidth="1.4"
              />
              <path
                d="M11.2 5.6 13.6 8l-2.4 2.4"
                stroke="currentColor"
                strokeWidth="1.4"
                fill="none"
              />
            </svg>
          </button>
          <button
            type="button"
            aria-label="Thu gọn sidebar"
            className="btnGhost"
            onClick={toggle}
            style={footerBtn}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
              <rect
                x="2"
                y="2.6"
                width="12"
                height="10.8"
                rx="2"
                stroke="currentColor"
                strokeWidth="1.4"
                fill="none"
              />
              <line
                x1="6.2"
                y1="2.6"
                x2="6.2"
                y2="13.4"
                stroke="currentColor"
                strokeWidth="1.4"
              />
            </svg>
          </button>
        </div>
      </div>
    </aside>
  );
}
