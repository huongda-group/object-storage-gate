// Ported from console-object-storage-gate/project/Dashboard.dc.html.
import { Link, createFileRoute, useRouter } from "@tanstack/react-router";
import type React from "react";
import { useState } from "react";
import { Header } from "../../components/Header";
import { useToast } from "../../components/Toast";
import { useShell } from "../../components/shell";
import {
  H1,
  Page,
  Panel,
  PanelHead,
  Pill,
  QuotaBar,
  monoStyle,
} from "../../components/ui";
import { endpoint, loadDashboard } from "../../lib/dashboard";
import { fmt, grp, pill, quotaView, shortId } from "../../lib/format";

export const Route = createFileRoute("/_app/")({
  loader: () => loadDashboard(),
  component: Dashboard,
});

type Tab = "aws" | "rclone" | "boto";

/// Built at render time from the deployment's own origin and one of the account's real bucket
/// names, so the command a user copies actually works against this gateway.
function snippets(host: string, bucket: string): Record<Tab, string> {
  return {
    aws: `aws configure set aws_access_key_id <ACCESS_KEY_ID>
aws s3 ls s3://${bucket}/ \\
  --endpoint-url ${host}`,
    rclone: `rclone config create osgate s3 \\
  provider=Other env_auth=false \\
  access_key_id=<ACCESS_KEY_ID> \\
  endpoint=${host}

rclone ls osgate:${bucket}`,
    boto: `import boto3

s3 = boto3.client("s3",
    endpoint_url="${host}",
    aws_access_key_id="<ACCESS_KEY_ID>",
    aws_secret_access_key="<SECRET>")

print(s3.list_objects_v2(Bucket="${bucket}")["KeyCount"])`,
  };
}

const TABS: { key: Tab; label: string }[] = [
  { key: "aws", label: "aws-cli" },
  { key: "rclone", label: "rclone" },
  { key: "boto", label: "boto3" },
];

function hhmm(d: Date) {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

const statCard: React.CSSProperties = {
  background: "var(--panel)",
  border: "1px solid var(--line)",
  borderRadius: 12,
  padding: "16px 18px",
};

const statLabel: React.CSSProperties = {
  fontSize: 11,
  letterSpacing: ".1em",
  color: "var(--faint)",
  fontWeight: 600,
};

const statValue: React.CSSProperties = {
  fontSize: 28,
  fontWeight: 600,
  letterSpacing: "-.03em",
  marginTop: 10,
  fontFamily: "'IBM Plex Mono',monospace",
};

function Dashboard() {
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const router = useRouter();
  const { summary, buckets, keys } = Route.useLoaderData();
  const [tab, setTab] = useState<Tab>("aws");
  const [copied, setCopied] = useState(false);
  const [syncAt, setSyncAt] = useState(() => hhmm(new Date()));

  const host = endpoint();
  const acc = quotaView(
    summary.used_bytes,
    summary.max_bytes,
    summary.reserved_bytes,
  );
  const sorted = [...buckets].sort((a, b) => b.used_bytes - a.used_bytes);
  const topBuckets = sorted.slice(0, 4);
  const recentKeys = keys.slice(0, 3);
  const isEmpty = buckets.length === 0;
  const sampleBucket = sorted[0]?.name ?? "<BUCKET>";
  const SNIPPETS = snippets(host, sampleBucket);

  const header = (
    <Header
      user={user}
      onLogout={requestLogout}
      left={
        <div style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
          Dashboard
        </div>
      }
    />
  );

  if (isEmpty) {
    return (
      <>
        {header}
        <Page>
          <div style={{ maxWidth: 760 }}>
            <H1>Chào mừng tới Object Storage Gate</H1>
            <p
              style={{
                color: "var(--dim)",
                fontSize: 14,
                margin: "8px 0 26px",
                maxWidth: "52ch",
                textWrap: "pretty",
              }}
            >
              Tài khoản của bạn chưa có bucket nào. Ba bước dưới đây mất chừng
              hai phút, sau đó bạn nói chuyện với gateway bằng aws-cli hoặc SDK
              như với S3 thật.
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <OnboardStep
                n={1}
                active
                title="Tạo bucket đầu tiên"
                text="Tên theo luật S3: chữ thường, số, dấu gạch ngang. Quota có thể để trống."
                cta={
                  <Link
                    to="/buckets"
                    style={{
                      alignSelf: "center",
                      height: 34,
                      padding: "0 14px",
                      borderRadius: 8,
                      background: "var(--acc)",
                      color: "var(--accTx)",
                      fontSize: 13,
                      fontWeight: 600,
                      display: "grid",
                      placeItems: "center",
                    }}
                  >
                    Tạo bucket
                  </Link>
                }
              />
              <OnboardStep
                n={2}
                title="Tạo access key"
                text="Chọn preset quyền Read-only hoặc Read/Write. Secret chỉ hiện một lần."
              />
              <OnboardStep
                n={3}
                title="Copy lệnh kết nối"
                text={
                  <>
                    Endpoint <span style={monoStyle}>{host}</span>.
                  </>
                }
              />
            </div>
          </div>
        </Page>
      </>
    );
  }

  return (
    <>
      {header}
      <Page>
        <div
          style={{
            display: "flex",
            alignItems: "flex-end",
            justifyContent: "space-between",
            marginBottom: 18,
          }}
        >
          <div>
            <H1>Dashboard</H1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              Số liệu là bản chụp, không realtime.
            </div>
          </div>
          <button
            type="button"
            className="btnGhost"
            onClick={() => {
              void router.invalidate().then(() => {
                setSyncAt(hhmm(new Date()));
                toast("Đã cập nhật số liệu quota");
              });
            }}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              height: 32,
              padding: "0 12px",
              border: "1px solid var(--line2)",
              background: "var(--panel)",
              color: "var(--dim)",
              borderRadius: 8,
              fontSize: 12,
              cursor: "pointer",
            }}
          >
            <svg width="13" height="13" viewBox="0 0 16 16" aria-hidden="true">
              <circle
                cx="8"
                cy="8"
                r="5.6"
                stroke="currentColor"
                strokeWidth="1.4"
                fill="none"
                strokeDasharray="24 9"
              />
              <path
                d="M12.4 2.6v3.6h-3.6"
                stroke="currentColor"
                strokeWidth="1.4"
                fill="none"
              />
            </svg>
            Cập nhật lúc {syncAt}
          </button>
        </div>

        <div
          data-grid="stats"
          style={{
            display: "grid",
            gridTemplateColumns: "1.5fr 1fr 1fr 1fr",
            gap: 14,
          }}
        >
          <div style={statCard}>
            <div style={statLabel}>DUNG LƯỢNG DÙNG</div>
            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                gap: 8,
                marginTop: 10,
              }}
            >
              <div
                style={{
                  fontSize: 28,
                  fontWeight: 600,
                  letterSpacing: "-.03em",
                  fontFamily: "'IBM Plex Mono',monospace",
                }}
              >
                {acc.usedText}
              </div>
              <div style={{ fontSize: 13, color: "var(--dim)" }}>
                / {acc.maxText}
              </div>
            </div>
            <div style={{ marginTop: 12 }}>
              <QuotaBar q={acc} />
            </div>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                fontSize: 12,
                color: "var(--dim)",
                marginTop: 8,
              }}
            >
              <span>{acc.pctText}</span>
              <span style={{ color: acc.color, fontWeight: 600 }}>
                {acc.state}
              </span>
            </div>
          </div>

          <div style={statCard}>
            <div style={statLabel}>BUCKET</div>
            <div style={statValue}>{summary.bucket_count}</div>
            <div style={{ fontSize: 12, color: "var(--dim)", marginTop: 6 }}>
              {buckets.filter((b) => b.max_bytes === 0).length} bucket không
              giới hạn
            </div>
          </div>

          <div style={statCard}>
            <div style={statLabel}>OBJECT</div>
            <div style={statValue}>{grp(summary.object_count)}</div>
            <div style={{ fontSize: 12, color: "var(--dim)", marginTop: 6 }}>
              trên {buckets.length} bucket
            </div>
          </div>

          <div style={statCard}>
            <div style={statLabel}>ACCESS KEY HOẠT ĐỘNG</div>
            <div style={statValue}>{summary.active_key_count}</div>
            <div style={{ fontSize: 12, color: "var(--warn)", marginTop: 6 }}>
              {
                keys.filter(
                  (k) =>
                    k.days_until_expiry !== null && k.days_until_expiry <= 3,
                ).length
              }{" "}
              key hết hạn trong 3 ngày
            </div>
          </div>
        </div>

        <div
          data-grid="two"
          style={{
            display: "grid",
            gridTemplateColumns: "1.35fr 1fr",
            gap: 14,
            marginTop: 14,
          }}
        >
          <Panel>
            <PanelHead
              title="Bucket dùng nhiều nhất"
              right={
                <Link to="/buckets" style={{ fontSize: 12, fontWeight: 500 }}>
                  Xem tất cả
                </Link>
              }
            />
            <div style={{ padding: "6px 8px" }}>
              {topBuckets.map((b) => {
                const q = quotaView(
                  b.used_bytes,
                  b.max_bytes,
                  b.reserved_bytes,
                );
                return (
                  <Link
                    key={b.name}
                    to="/buckets/$pid"
                    params={{ pid: b.pid }}
                    className="rowHover linkPlain"
                    style={{
                      display: "grid",
                      gridTemplateColumns: "minmax(110px,180px) 1fr auto",
                      alignItems: "center",
                      gap: 16,
                      width: "100%",
                      padding: "11px 10px",
                      borderRadius: 8,
                      textAlign: "left",
                    }}
                  >
                    <div
                      style={{
                        ...monoStyle,
                        fontSize: 13,
                        color: "var(--tx)",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {b.name}
                    </div>
                    <div>
                      {!q.unlimited && <QuotaBar q={q} height={5} />}
                      <div
                        style={{
                          fontSize: 11,
                          color: "var(--dim)",
                          marginTop: 5,
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {q.unlimited
                          ? `${fmt(b.used_bytes)} đã dùng · ∞ Không giới hạn`
                          : q.pctText}
                      </div>
                    </div>
                    <div
                      style={{
                        fontSize: 12,
                        color: "var(--dim)",
                        textAlign: "right",
                        ...monoStyle,
                        whiteSpace: "nowrap",
                      }}
                    >
                      {grp(b.object_count)} obj
                    </div>
                  </Link>
                );
              })}
            </div>
          </Panel>

          <Panel>
            <PanelHead
              title="Access key gần nhất"
              right={
                <Link to="/keys" style={{ fontSize: 12, fontWeight: 500 }}>
                  Xem tất cả
                </Link>
              }
            />
            <div style={{ padding: "6px 8px" }}>
              {recentKeys.map((k) => (
                <Link
                  key={k.pid}
                  to="/keys/$pid"
                  params={{ pid: k.pid }}
                  className="rowHover linkPlain"
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 12,
                    width: "100%",
                    padding: "11px 10px",
                    borderRadius: 8,
                    textAlign: "left",
                  }}
                >
                  <div style={{ minWidth: 0 }}>
                    <div
                      style={{
                        ...monoStyle,
                        fontSize: 12.5,
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {shortId(k.access_key_id)}
                    </div>
                    <div
                      style={{
                        fontSize: 11,
                        color: "var(--faint)",
                        marginTop: 3,
                      }}
                    >
                      {k.label} ·{" "}
                      {new Date(k.created_at).toLocaleDateString("vi-VN")}
                    </div>
                  </div>
                  <Pill view={pill(k.status)} />
                </Link>
              ))}
            </div>
          </Panel>
        </div>

        <Panel style={{ marginTop: 14 }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              padding: "10px 14px",
              borderBottom: "1px solid var(--line)",
            }}
          >
            <div style={{ fontSize: 13, fontWeight: 600, marginRight: 12 }}>
              Kết nối nhanh
            </div>
            {TABS.map((t) => (
              <button
                key={t.key}
                type="button"
                onClick={() => setTab(t.key)}
                style={{
                  height: 28,
                  padding: "0 12px",
                  border: 0,
                  borderRadius: 7,
                  fontSize: 12,
                  fontWeight: 500,
                  cursor: "pointer",
                  ...monoStyle,
                  background: tab === t.key ? "var(--accSoft)" : "transparent",
                  color: tab === t.key ? "var(--acc)" : "var(--dim)",
                }}
              >
                {t.label}
              </button>
            ))}
            <div style={{ flex: 1 }} />
            <button
              type="button"
              className="btnGhost"
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(SNIPPETS[tab]);
                } catch {
                  toast(
                    "Trình duyệt không cho copy. Chọn và copy thủ công.",
                    "danger",
                  );
                  return;
                }
                setCopied(true);
                toast("Đã copy vào clipboard");
                setTimeout(() => setCopied(false), 2600);
              }}
              style={{
                height: 28,
                padding: "0 12px",
                border: "1px solid var(--line2)",
                background: "var(--panel2)",
                color: "var(--dim)",
                borderRadius: 7,
                fontSize: 12,
                cursor: "pointer",
              }}
            >
              {copied ? "✓" : "⧉"} Copy
            </button>
          </div>
          <pre
            style={{
              margin: 0,
              padding: "16px 18px",
              ...monoStyle,
              fontSize: 12.5,
              lineHeight: 1.75,
              color: "#D8D2C8",
              overflowX: "auto",
              background: "#0C0B0A",
            }}
          >
            {SNIPPETS[tab]}
          </pre>
        </Panel>
      </Page>
    </>
  );
}

function OnboardStep({
  n,
  title,
  text,
  cta,
  active,
}: {
  n: number;
  title: string;
  text: React.ReactNode;
  cta?: React.ReactNode;
  active?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        gap: 16,
        background: "var(--panel)",
        border: `1px solid ${active ? "var(--accLine)" : "var(--line)"}`,
        borderRadius: 12,
        padding: "18px 20px",
        opacity: active ? 1 : 0.72,
      }}
    >
      <div
        style={{
          width: 26,
          height: 26,
          flex: "0 0 26px",
          borderRadius: "50%",
          background: active ? "var(--acc)" : "var(--panel2)",
          border: active ? undefined : "1px solid var(--line2)",
          color: active ? "var(--accTx)" : "var(--dim)",
          display: "grid",
          placeItems: "center",
          fontSize: 13,
          fontWeight: 700,
        }}
      >
        {n}
      </div>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 14, fontWeight: 600 }}>{title}</div>
        <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 4 }}>
          {text}
        </div>
      </div>
      {cta}
    </div>
  );
}
