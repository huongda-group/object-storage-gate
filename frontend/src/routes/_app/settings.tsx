// Ported from console-object-storage-gate/project/Settings.dc.html.
// The starter backend has no profile-update or change-password endpoint, so both submits stay disabled (same "Sắp có" treatment as Upload in Bucket Detail).
import { createFileRoute } from "@tanstack/react-router";
import type React from "react";
import { useState } from "react";
import { Header } from "../../components/Header";
import { useToast } from "../../components/Toast";
import { useShell } from "../../components/shell";
import { H1, Page } from "../../components/ui";
import { run } from "../../lib/api-client";
import { changePassword } from "../../lib/auth";
import { updateMe } from "../../lib/me";

export const Route = createFileRoute("/_app/settings")({ component: Settings });

const fieldLabel: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 7,
  fontSize: 12,
  fontWeight: 500,
  color: "var(--dim)",
};

const input: React.CSSProperties = {
  height: 36,
  borderRadius: 8,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--tx)",
  padding: "0 12px",
  fontSize: 14,
};

const card: React.CSSProperties = {
  background: "var(--panel)",
  border: "1px solid var(--line)",
  borderRadius: 12,
  padding: 20,
};

function SaveButton({
  label,
  enabled,
  onClick,
}: {
  label: string;
  enabled: boolean;
  onClick: () => void;
}) {
  return (
    <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 18 }}>
      <button
        type="button"
        disabled={!enabled}
        onClick={onClick}
        style={{
          height: 34,
          padding: "0 16px",
          border: 0,
          borderRadius: 8,
          background: enabled ? "var(--acc)" : "var(--panel2)",
          color: enabled ? "var(--accTx)" : "var(--faint)",
          fontSize: 13,
          fontWeight: 600,
          cursor: enabled ? "pointer" : "not-allowed",
        }}
      >
        {label}
      </button>
    </div>
  );
}

function Settings() {
  const { user, requestLogout } = useShell();
  const [name, setName] = useState(user.name);
  const [pwCurrent, setPwCurrent] = useState("");
  const [pwNew, setPwNew] = useState("");
  const [pwConfirm, setPwConfirm] = useState("");

  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const longEnough = pwNew.length >= 8;
  const matches = pwNew.length > 0 && pwNew === pwConfirm;

  async function saveProfile() {
    if (busy) return;
    setBusy(true);
    const updated = await run(() => updateMe(name.trim()), {
      onError: (m) => toast(m, "danger"),
    });
    setBusy(false);
    // A full reload drops the cached user, which still carries the old name in the header.
    if (updated) globalThis.location.reload();
  }

  async function savePassword() {
    if (busy) return;
    setBusy(true);
    const ok = await run(() => changePassword(pwCurrent, pwNew), {
      onError: (m) => toast(m, "danger"),
    });
    setBusy(false);
    if (ok === undefined) return;
    setPwCurrent("");
    setPwNew("");
    setPwConfirm("");
    toast("Đã đổi mật khẩu");
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            Cài đặt
          </div>
        }
      />
      <Page>
        <H1>Cài đặt</H1>
        <div style={{ height: 20 }} />
        <div
          style={{
            maxWidth: 1000,
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: 14,
            alignItems: "start",
          }}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <div style={card}>
              <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 16 }}>
                Hồ sơ
              </div>
              <div
                style={{ display: "flex", flexDirection: "column", gap: 14 }}
              >
                <label style={fieldLabel}>
                  Tên
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    style={input}
                  />
                </label>
                <label style={fieldLabel}>
                  Email (không đổi được)
                  <input
                    value={user.email}
                    readOnly
                    style={{
                      ...input,
                      border: "1px solid var(--line)",
                      background: "#131211",
                      color: "var(--faint)",
                    }}
                  />
                </label>
              </div>
              <SaveButton
                label="Lưu hồ sơ"
                enabled={name.trim().length >= 2 && !busy}
                onClick={() => void saveProfile()}
              />
            </div>
          </div>

          <div style={card}>
            <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 16 }}>
              Đổi mật khẩu
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
              <label style={fieldLabel}>
                Mật khẩu hiện tại
                <input
                  type="password"
                  autoComplete="current-password"
                  value={pwCurrent}
                  onChange={(e) => setPwCurrent(e.target.value)}
                  style={input}
                />
              </label>
              <label style={fieldLabel}>
                Mật khẩu mới
                <input
                  type="password"
                  autoComplete="new-password"
                  value={pwNew}
                  onChange={(e) => setPwNew(e.target.value)}
                  style={input}
                />
              </label>
              <label style={fieldLabel}>
                Xác nhận mật khẩu mới
                <input
                  type="password"
                  autoComplete="new-password"
                  value={pwConfirm}
                  onChange={(e) => setPwConfirm(e.target.value)}
                  style={input}
                />
              </label>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  fontSize: 12.5,
                  color: "var(--dim)",
                }}
              >
                <span
                  style={{ color: longEnough ? "var(--ok)" : "var(--faint)" }}
                >
                  ✓
                </span>{" "}
                Tối thiểu 8 ký tự
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  fontSize: 12.5,
                  color: "var(--dim)",
                }}
              >
                <span style={{ color: matches ? "var(--ok)" : "var(--faint)" }}>
                  ✓
                </span>{" "}
                Hai mật khẩu khớp nhau
              </div>
            </div>
            <SaveButton
              label="Đổi mật khẩu"
              enabled={longEnough && matches && pwCurrent.length > 0 && !busy}
              onClick={() => void savePassword()}
            />
          </div>
        </div>
      </Page>
    </>
  );
}
