// Styles copied verbatim from console-object-storage-gate/project/Object Storage Gate.dc.html.
import type React from "react";
import { useState } from "react";

export function Title({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ fontSize: 22, fontWeight: 600, letterSpacing: "-.015em" }}>
      {children}
    </div>
  );
}

export function Sub({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 13.5,
        color: "var(--dim)",
        marginTop: 7,
        lineHeight: 1.55,
      }}
    >
      {children}
    </div>
  );
}

export function ErrorBanner({ children }: { children: React.ReactNode }) {
  return (
    <div
      role="alert"
      style={{
        marginTop: 18,
        display: "flex",
        gap: 10,
        alignItems: "flex-start",
        background: "var(--dgrSoft)",
        border: "1px solid rgba(232,82,94,.4)",
        borderRadius: 9,
        padding: "11px 13px",
      }}
    >
      <div
        style={{
          width: 15,
          height: 15,
          flex: "0 0 15px",
          borderRadius: "50%",
          background: "var(--dgr)",
          color: "#1A0D03",
          fontSize: 10,
          fontWeight: 700,
          display: "grid",
          placeItems: "center",
          marginTop: 1,
        }}
      >
        !
      </div>
      <div style={{ fontSize: 13, color: "#FF9AA2", lineHeight: 1.45 }}>
        {children}
      </div>
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  height: 40,
  borderRadius: 9,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--tx)",
  padding: "0 12px",
  fontSize: 14,
  outline: "none",
};

const labelStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 7,
  fontSize: 12,
  fontWeight: 500,
  color: "var(--dim)",
};

export function TextField({
  label,
  type = "text",
  value,
  onChange,
  onEnter,
  placeholder,
  autoComplete,
  disabled,
}: {
  label: string;
  type?: string;
  value: string;
  onChange: (v: string) => void;
  onEnter?: () => void;
  placeholder?: string;
  autoComplete?: string;
  disabled?: boolean;
}) {
  return (
    <label style={labelStyle}>
      {label}
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        autoComplete={autoComplete}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && onEnter) onEnter();
        }}
        style={disabled ? { ...inputStyle, color: "var(--faint)" } : inputStyle}
      />
    </label>
  );
}

export function PasswordField({
  label,
  value,
  onChange,
  onEnter,
  autoComplete = "current-password",
  right,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  onEnter?: () => void;
  autoComplete?: string;
  right?: React.ReactNode;
}) {
  const [show, setShow] = useState(false);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <span style={{ fontSize: 12, fontWeight: 500, color: "var(--dim)" }}>
          {label}
        </span>
        {right}
      </div>
      <div style={{ position: "relative", display: "flex" }}>
        <input
          type={show ? "text" : "password"}
          autoComplete={autoComplete}
          placeholder="••••••••"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && onEnter) onEnter();
          }}
          style={{ ...inputStyle, flex: 1, padding: "0 74px 0 12px" }}
        />
        <button
          type="button"
          className="btnGhost"
          onClick={() => setShow(!show)}
          style={{
            position: "absolute",
            right: 6,
            top: 6,
            height: 28,
            padding: "0 9px",
            border: "1px solid var(--line2)",
            background: "var(--panel)",
            color: "var(--dim)",
            borderRadius: 7,
            fontSize: 11,
            fontWeight: 500,
            cursor: "pointer",
            fontFamily: "'IBM Plex Mono',monospace",
          }}
        >
          {show ? "ẨN" : "HIỆN"}
        </button>
      </div>
    </div>
  );
}

export function SubmitButton({
  busy,
  label,
  busyLabel,
  onClick,
}: {
  busy: boolean;
  label: string;
  busyLabel: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={busy}
      style={{
        height: 42,
        border: 0,
        borderRadius: 9,
        background: "var(--acc)",
        color: "var(--accTx)",
        fontWeight: 600,
        fontSize: 14,
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 9,
        opacity: busy ? 0.75 : 1,
      }}
    >
      {busy && (
        <span
          style={{
            width: 13,
            height: 13,
            borderRadius: "50%",
            border: "2px solid rgba(26,13,3,.35)",
            borderTopColor: "#1A0D03",
            animation: "spin .7s linear infinite",
          }}
        />
      )}
      {busy ? busyLabel : label}
    </button>
  );
}

export function SecondaryButton({
  label,
  onClick,
}: {
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="btnGhost"
      onClick={onClick}
      style={{
        height: 40,
        border: "1px solid var(--line2)",
        borderRadius: 9,
        background: "var(--panel2)",
        color: "var(--dim)",
        fontWeight: 500,
        fontSize: 13.5,
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}

export function Mark({ kind }: { kind: "ok" | "bad" }) {
  const ok = kind === "ok";
  return (
    <div
      style={{
        width: 34,
        height: 34,
        borderRadius: 10,
        background: ok ? "rgba(63,181,121,.13)" : "var(--dgrSoft)",
        border: `1px solid ${ok ? "rgba(63,181,121,.4)" : "rgba(232,82,94,.4)"}`,
        color: ok ? "var(--ok)" : "var(--dgr)",
        display: "grid",
        placeItems: "center",
        fontSize: 15,
        fontWeight: 700,
      }}
    >
      {ok ? "✓" : "!"}
    </div>
  );
}

export const fieldStack: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 15,
  marginTop: 22,
};

export const mono: React.CSSProperties = {
  fontFamily: "'IBM Plex Mono',monospace",
  color: "var(--tx)",
};
