// Header, avatar menu and logout confirm from
// console-object-storage-gate/project/Dashboard.dc.html (lines 44-62, 200-211).
import { Link } from "@tanstack/react-router";
import type React from "react";
import { useState } from "react";
import type { CurrentUser } from "../lib/auth";

export function initialsOf(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

const menuItem: React.CSSProperties = {
  display: "block",
  width: "100%",
  textAlign: "left",
  padding: "8px 10px",
  borderRadius: 7,
  fontSize: 13,
  color: "var(--tx)",
  background: "none",
  border: 0,
  cursor: "pointer",
};

export function Header({
  left,
  right,
  user,
  onLogout,
}: {
  left: React.ReactNode;
  right?: React.ReactNode;
  user: CurrentUser;
  onLogout: () => void;
}) {
  const [open, setOpen] = useState(false);

  return (
    <header
      style={{
        height: 56,
        flex: "0 0 56px",
        borderBottom: "1px solid var(--line)",
        background: "rgba(16,15,14,.86)",
        backdropFilter: "blur(8px)",
        position: "sticky",
        top: 0,
        zIndex: 30,
        display: "flex",
        alignItems: "center",
        gap: 16,
        padding: "0 24px",
      }}
    >
      {left}
      <div style={{ flex: 1 }} />
      {right}
      <div style={{ position: "relative" }}>
        <button
          type="button"
          onClick={() => setOpen(!open)}
          aria-label="Tài khoản"
          style={{
            width: 30,
            height: 30,
            borderRadius: "50%",
            background: "var(--accSoft)",
            border: "1px solid var(--accLine)",
            color: "var(--acc)",
            display: "grid",
            placeItems: "center",
            fontSize: 11,
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          {initialsOf(user.name)}
        </button>
        {open && (
          <>
            <button
              type="button"
              aria-label="Đóng menu tài khoản"
              onClick={() => setOpen(false)}
              style={{
                position: "fixed",
                inset: 0,
                zIndex: 39,
                border: 0,
                background: "transparent",
                cursor: "default",
              }}
            />
            <div
              style={{
                position: "absolute",
                top: 38,
                right: 0,
                zIndex: 40,
                width: 200,
                background: "var(--panel2)",
                border: "1px solid var(--line2)",
                borderRadius: 10,
                boxShadow: "0 14px 34px rgba(0,0,0,.5)",
                overflow: "hidden",
                padding: 6,
              }}
            >
              <div
                style={{
                  padding: "8px 10px",
                  borderBottom: "1px solid var(--line)",
                }}
              >
                <div
                  style={{
                    fontSize: 12.5,
                    fontWeight: 500,
                    color: "var(--tx)",
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {user.email}
                </div>
                <div
                  style={{
                    fontSize: 10,
                    color: user.role === "admin" ? "var(--info)" : "var(--dim)",
                    fontWeight: 600,
                    letterSpacing: ".06em",
                    marginTop: 2,
                  }}
                >
                  {user.role.toUpperCase()}
                </div>
              </div>
              <Link
                to="/profile"
                className="rowHover"
                style={{ ...menuItem, marginTop: 4 }}
                onClick={() => setOpen(false)}
              >
                Hồ sơ
              </Link>
              <Link
                to="/settings"
                className="rowHover"
                style={menuItem}
                onClick={() => setOpen(false)}
              >
                Cài đặt
              </Link>
              <button
                type="button"
                className="rowHover"
                onClick={() => {
                  setOpen(false);
                  onLogout();
                }}
                style={{ ...menuItem, color: "var(--dgr)" }}
              >
                Đăng xuất
              </button>
            </div>
          </>
        )}
      </div>
    </header>
  );
}
