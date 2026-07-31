const UNITS = ["B", "KiB", "MiB", "GiB", "TiB"];

export function fmt(n: number): string {
  if (!n) return "0 B";
  let i = 0;
  let v = n;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const text =
    i === 0 ? String(Math.round(v)) : v.toFixed(1).replace(/\.0$/, "");
  return `${text} ${UNITS[i]}`;
}

export function grp(n: number): string {
  return n.toLocaleString("en-US").replace(/,/g, ".");
}

export function colorFor(pct: number): string {
  if (pct >= 90) return "var(--dgr)";
  if (pct >= 75) return "var(--warn)";
  return "var(--ok)";
}

export type QuotaView = {
  unlimited: boolean;
  usedText: string;
  maxText: string;
  pctText: string;
  /** compact "42.7 GiB / 50 GiB" (or "12.4 GiB · ∞") for table rows */
  usedLine: string;
  barW: string;
  resW: string;
  color: string;
  state: string;
};

export function quotaView(used: number, max: number, res = 0): QuotaView {
  if (!max) {
    return {
      unlimited: true,
      usedText: fmt(used),
      maxText: "Không giới hạn",
      pctText: `${fmt(used)} đã dùng · ∞ Không giới hạn`,
      usedLine: `${fmt(used)} · ∞`,
      barW: "0%",
      resW: "0%",
      color: "var(--acc)",
      state: "Không giới hạn",
    };
  }
  const pct = Math.min(100, (used / max) * 100);
  const rp = Math.min(100 - pct, (res / max) * 100);
  return {
    unlimited: false,
    usedText: fmt(used),
    maxText: fmt(max),
    pctText: `${fmt(used)} / ${fmt(max)} (${pct.toFixed(1)}%)`,
    usedLine: `${fmt(used)} / ${fmt(max)}`,
    barW: `${pct.toFixed(1)}%`,
    resW: `${rp.toFixed(1)}%`,
    color: colorFor(pct),
    state: pct >= 90 ? "Sắp đầy" : `Còn ${fmt(max - used)}`,
  };
}

export type KeyStatus = "active" | "disabled" | "expired" | "revoked";
export type PillView = { pill: string; pillBg: string; pillFg: string };

export function pill(status: KeyStatus): PillView {
  if (status === "active")
    return {
      pill: "Đang hoạt động",
      pillBg: "var(--okSoft)",
      pillFg: "var(--ok)",
    };
  if (status === "disabled")
    return { pill: "Tạm khoá", pillBg: "var(--panel2)", pillFg: "var(--dim)" };
  if (status === "expired")
    return { pill: "Hết hạn", pillBg: "var(--accSoft)", pillFg: "var(--acc)" };
  return { pill: "Đã thu hồi", pillBg: "var(--dgrSoft)", pillFg: "var(--dgr)" };
}

export function shortId(id: string): string {
  return `${id.slice(0, 7)}…${id.slice(-4)}`;
}
