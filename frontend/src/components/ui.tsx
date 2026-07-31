// Small shared pieces lifted from the prototypes: quota bars, status pills,
// copy buttons, page/panel chrome.
import type React from "react";
import { useState } from "react";
import type { PillView, QuotaView } from "../lib/format";
import type { UNITS } from "../lib/mock";
import { useToast } from "./Toast";

/**
 * Row action menu (`⋯`). The table scrolls horizontally, so an absolutely
 * positioned menu would clip — the prototypes measure the button and place the
 * menu with `position: fixed`, which is what this reproduces.
 */
export function useRowMenu() {
  const [open, setOpen] = useState<string | null>(null);
  const [pos, setPos] = useState<{ top: number; right: number }>({
    top: 0,
    right: 0,
  });

  function toggle(
    id: string,
    e: React.MouseEvent<HTMLElement>,
    height: number,
  ) {
    if (open === id) {
      setOpen(null);
      return;
    }
    const r = e.currentTarget.getBoundingClientRect();
    let top = r.bottom + 6;
    if (top + height > window.innerHeight - 12) {
      top = Math.max(12, r.top - height - 6);
    }
    setPos({ top, right: Math.max(12, window.innerWidth - r.right - 2) });
    setOpen(id);
  }

  return { open, pos, toggle, close: () => setOpen(null) };
}

export function RowMenuButton({
  onClick,
}: {
  onClick: (e: React.MouseEvent<HTMLElement>) => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label="Hành động"
      className="iconBtn"
      style={{
        width: 28,
        height: 28,
        border: 0,
        background: "none",
        color: "var(--dim)",
        borderRadius: 6,
        cursor: "pointer",
        fontSize: 15,
      }}
    >
      ⋯
    </button>
  );
}

export function RowMenu({
  pos,
  children,
}: {
  pos: { top: number; right: number };
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        position: "fixed",
        right: pos.right,
        top: pos.top,
        zIndex: 90,
        background: "var(--panel2)",
        border: "1px solid var(--line2)",
        borderRadius: 9,
        boxShadow: "0 12px 30px rgba(0,0,0,.5)",
        padding: 5,
        width: 170,
        textAlign: "left",
      }}
    >
      {children}
    </div>
  );
}

export const menuItemStyle: React.CSSProperties = {
  display: "block",
  width: "100%",
  textAlign: "left",
  padding: "8px 10px",
  color: "var(--tx)",
  fontSize: 13,
  borderRadius: 6,
  background: "none",
  border: 0,
  cursor: "pointer",
};

/** Table shell: scroll container + panel chrome. */
export function TableWrap({ children }: { children: React.ReactNode }) {
  return (
    <div
      data-tscroll=""
      style={{
        background: "var(--panel)",
        border: "1px solid var(--line)",
        borderRadius: 12,
      }}
    >
      {children}
    </div>
  );
}

export function Th({
  children,
  align = "left",
  width,
}: {
  children?: React.ReactNode;
  align?: "left" | "right" | "center";
  width?: number;
}) {
  return (
    <th
      scope="col"
      style={{
        textAlign: align,
        fontSize: 11,
        letterSpacing: ".08em",
        color: "var(--faint)",
        fontWeight: 600,
        padding: "0 16px",
        height: 38,
        background: "var(--panel2)",
        borderBottom: "1px solid var(--line)",
        width,
      }}
    >
      {children}
    </th>
  );
}

export function Td({
  children,
  align = "left",
  style,
  title,
}: {
  children?: React.ReactNode;
  align?: "left" | "right" | "center";
  style?: React.CSSProperties;
  title?: string;
}) {
  return (
    <td
      title={title}
      style={{
        padding: "0 16px",
        height: "var(--row)",
        textAlign: align,
        ...style,
      }}
    >
      {children}
    </td>
  );
}

/** Footer row under a table: "Hiện 1–N trong N" + per-page select. */
export function TableFoot({ shown, total }: { shown: number; total: number }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "12px 16px",
        borderTop: "1px solid var(--line)",
        background: "var(--panel2)",
      }}
    >
      <div style={{ fontSize: 12, color: "var(--faint)" }}>
        {shown === 0 ? "Không có dòng nào" : `Hiện 1–${shown} trong ${total}`}
      </div>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: 12,
          color: "var(--faint)",
        }}
      >
        <span>Mỗi trang</span>
        {/* TODO(slice#7): server-side paging — the select is inert until the API pages. */}
        <select
          disabled
          style={{
            height: 28,
            borderRadius: 6,
            border: "1px solid var(--line2)",
            background: "var(--panel)",
            color: "var(--dim)",
            fontSize: 12,
            padding: "0 6px",
          }}
        >
          <option>25</option>
          <option>50</option>
          <option>100</option>
        </select>
      </div>
    </div>
  );
}

/** Centred empty state inside a table panel. */
export function TableEmpty({
  title,
  text,
  action,
}: {
  title: string;
  text: string;
  action?: React.ReactNode;
}) {
  return (
    <div style={{ padding: "64px 24px", textAlign: "center" }}>
      <div
        style={{
          width: 40,
          height: 40,
          borderRadius: 12,
          border: "1px solid var(--line2)",
          background: "var(--panel2)",
          margin: "0 auto 16px",
        }}
      />
      <div style={{ fontSize: 14, fontWeight: 600 }}>{title}</div>
      <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 6 }}>
        {text}
      </div>
      {action && <div style={{ marginTop: 18 }}>{action}</div>}
    </div>
  );
}

/** Primary page action, e.g. "Tạo bucket" in the page header row. */
export function PageAction({
  label,
  onClick,
}: {
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        height: 36,
        padding: "0 16px",
        border: 0,
        borderRadius: 8,
        background: "var(--acc)",
        color: "var(--accTx)",
        fontSize: 13,
        fontWeight: 600,
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}

/** Two-segment quota bar: solid = used, 45° stripes = reserved. */
export function QuotaBar({
  q,
  height = 8,
}: {
  q: QuotaView;
  height?: number;
}) {
  return (
    <div
      style={{
        height,
        borderRadius: height / 2,
        background: "var(--line)",
        overflow: "hidden",
        display: "flex",
      }}
    >
      <div style={{ width: q.barW, background: q.color }} />
      {q.resW !== "0.0%" && q.resW !== "0%" && (
        <div
          style={{
            width: q.resW,
            background: `repeating-linear-gradient(45deg,${q.color} 0 3px,transparent 3px 6px)`,
            opacity: 0.6,
          }}
        />
      )}
    </div>
  );
}

export function Pill({ view }: { view: PillView }) {
  return (
    <span
      style={{
        flex: "0 0 auto",
        fontSize: 11,
        fontWeight: 600,
        padding: "3px 8px",
        borderRadius: 20,
        whiteSpace: "nowrap",
        background: view.pillBg,
        color: view.pillFg,
      }}
    >
      {view.pill}
    </span>
  );
}

/** Status pill with a leading dot — the keys table variant. */
export function PillDot({ view }: { view: PillView }) {
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        fontSize: 12,
        fontWeight: 600,
        padding: "4px 9px",
        borderRadius: 20,
        whiteSpace: "nowrap",
        background: view.pillBg,
        color: view.pillFg,
      }}
    >
      <span
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: view.pillFg,
        }}
      />
      {view.pill}
    </span>
  );
}

/** Monospace chip — permission and label chips in the keys table. */
export function Chip({
  children,
  tone = "acc",
}: {
  children: React.ReactNode;
  tone?: "acc" | "dim" | "faint";
}) {
  const tones = {
    acc: { background: "var(--accSoft)", color: "var(--acc)", border: "none" },
    dim: {
      background: "var(--panel2)",
      color: "var(--dim)",
      border: "1px solid var(--line2)",
    },
    faint: {
      background: "var(--panel2)",
      color: "var(--faint)",
      border: "none",
    },
  } as const;
  return (
    <span
      style={{
        fontSize: 11,
        padding: "3px 7px",
        borderRadius: 5,
        fontFamily: "'IBM Plex Mono',monospace",
        ...tones[tone],
      }}
    >
      {children}
    </span>
  );
}

/** Copy-to-clipboard button that flips to ✓ and raises the toast. */
export function Copyable({
  value,
  label,
  style,
}: {
  value: string;
  label?: string;
  style?: React.CSSProperties;
}) {
  const toast = useToast();
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard?.writeText(value);
    } catch {
      // clipboard blocked (insecure origin / denied) — still show the state
    }
    setCopied(true);
    toast("Đã copy vào clipboard");
    setTimeout(() => setCopied(false), 2600);
  }

  return (
    <button
      type="button"
      className="btnGhost"
      onClick={copy}
      aria-label={`Copy ${label ?? value}`}
      style={{
        height: 28,
        padding: "0 9px",
        border: "1px solid var(--line2)",
        background: "var(--panel2)",
        color: "var(--dim)",
        borderRadius: 7,
        fontSize: 12,
        cursor: "pointer",
        ...style,
      }}
    >
      {copied ? "✓" : "⧉"}
      {label ? ` ${label}` : ""}
    </button>
  );
}

/** Page content wrapper — Dashboard.dc.html line 64. */
export function Page({
  children,
  maxWidth = 1400,
}: {
  children: React.ReactNode;
  maxWidth?: number;
}) {
  return (
    <div
      style={{
        flex: 1,
        padding: 24,
        maxWidth,
        width: "100%",
        margin: "0 auto",
      }}
    >
      {children}
    </div>
  );
}

export function Panel({
  children,
  style,
}: {
  children: React.ReactNode;
  style?: React.CSSProperties;
}) {
  return (
    <div
      style={{
        background: "var(--panel)",
        border: "1px solid var(--line)",
        borderRadius: 12,
        overflow: "hidden",
        ...style,
      }}
    >
      {children}
    </div>
  );
}

export function PanelHead({
  title,
  right,
}: {
  title: React.ReactNode;
  right?: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "14px 18px",
        borderBottom: "1px solid var(--line)",
      }}
    >
      <div style={{ fontSize: 13, fontWeight: 600 }}>{title}</div>
      {right}
    </div>
  );
}

export function H1({ children }: { children: React.ReactNode }) {
  return (
    <h1
      style={{
        fontSize: 22,
        fontWeight: 600,
        letterSpacing: "-.02em",
        margin: 0,
      }}
    >
      {children}
    </h1>
  );
}

export const monoStyle: React.CSSProperties = {
  fontFamily: "'IBM Plex Mono',monospace",
};

/** Header search box — Buckets.dc.html / Admin Users.dc.html. */
export function HeaderSearch({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
}) {
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      aria-label={placeholder}
      style={{
        width: 280,
        height: 32,
        borderRadius: 8,
        border: "1px solid var(--line2)",
        background: "var(--panel)",
        color: "var(--tx)",
        padding: "0 12px",
        fontSize: 13,
      }}
    />
  );
}

/** number + unit + "Không giới hạn" trio, shared by every quota form. */
export function QuotaFields({
  num,
  unit,
  unlimited,
  onNum,
  onUnit,
  onUnlimited,
}: {
  num: string;
  unit: keyof typeof UNITS;
  unlimited: boolean;
  onNum: (v: string) => void;
  onUnit: (v: keyof typeof UNITS) => void;
  onUnlimited: (v: boolean) => void;
}) {
  return (
    <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
      <input
        value={num}
        onChange={(e) => onNum(e.target.value)}
        disabled={unlimited}
        style={{
          width: 120,
          height: 38,
          borderRadius: 8,
          border: "1px solid var(--line2)",
          background: "var(--panel2)",
          color: unlimited ? "var(--faint)" : "var(--tx)",
          padding: "0 12px",
          fontSize: 14,
          fontFamily: "'IBM Plex Mono',monospace",
        }}
      />
      <select
        value={unit}
        onChange={(e) => onUnit(e.target.value as keyof typeof UNITS)}
        disabled={unlimited}
        style={{
          height: 38,
          borderRadius: 8,
          border: "1px solid var(--line2)",
          background: "var(--panel2)",
          color: "var(--tx)",
          padding: "0 10px",
          fontSize: 13,
        }}
      >
        <option>MiB</option>
        <option>GiB</option>
        <option>TiB</option>
      </select>
      <label
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: 13,
          color: "var(--dim)",
          cursor: "pointer",
        }}
      >
        <input
          type="checkbox"
          checked={unlimited}
          onChange={(e) => onUnlimited(e.target.checked)}
          style={{
            accentColor: "var(--acc)",
            width: 14,
            height: 14,
            cursor: "pointer",
          }}
        />
        Không giới hạn
      </label>
    </div>
  );
}
