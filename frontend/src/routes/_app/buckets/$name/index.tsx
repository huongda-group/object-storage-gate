// Ported from console-object-storage-gate/project/Bucket Detail.dc.html.
import { Link, createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { Header } from "../../../../components/Header";
import { useToast } from "../../../../components/Toast";
import { useShell } from "../../../../components/shell";
import {
  HeaderSearch,
  Page,
  TableEmpty,
  TableWrap,
  Td,
  Th,
  monoStyle,
} from "../../../../components/ui";
import { fmt, grp, quotaView } from "../../../../lib/format";
import { BUCKETS, ENDPOINT, OBJECTS, REGION } from "../../../../lib/mock";

export const Route = createFileRoute("/_app/buckets/$name/")({
  component: BucketDetail,
});

type Row = {
  kind: "dir" | "file";
  label: string;
  /** full object key (or prefix), used for selection, copy and navigation */
  key: string;
  size: string;
  type: string;
  etag: string;
  updated: string;
};

/** Group the flat key list into the current prefix's folders and files. */
function rowsFor(prefix: string): Row[] {
  const scoped = OBJECTS.filter((o) => o.key.startsWith(prefix));
  const dirs = new Map<string, number>();
  const files: Row[] = [];

  for (const o of scoped) {
    const rest = o.key.slice(prefix.length);
    const slash = rest.indexOf("/");
    if (slash >= 0) {
      const dir = rest.slice(0, slash + 1);
      dirs.set(dir, (dirs.get(dir) ?? 0) + 1);
    } else {
      files.push({
        kind: "file",
        label: rest,
        key: o.key,
        size: fmt(o.size),
        type: o.contentType,
        etag: `${o.etag}…`,
        updated: o.updated,
      });
    }
  }

  const dirRows: Row[] = [...dirs].map(([dir, count]) => ({
    kind: "dir",
    label: dir,
    key: prefix + dir,
    size: "—",
    type: "",
    etag: "—",
    updated: `${count} object`,
  }));

  return [...dirRows, ...files];
}

function BucketDetail() {
  const { name } = Route.useParams();
  const { user, requestLogout } = useShell();
  const toast = useToast();

  const [prefix, setPrefix] = useState("");
  const [prefixQuery, setPrefixQuery] = useState("");
  const [sel, setSel] = useState<string[]>([]);
  const [copied, setCopied] = useState<string | null>(null);
  const [deleted, setDeleted] = useState<string[]>([]);

  const bucket = BUCKETS.find((b) => b.name === name);
  const activePrefix = prefixQuery || prefix;

  const header = (
    <Header
      user={user}
      onLogout={requestLogout}
      left={
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            minWidth: 0,
          }}
        >
          <Link to="/buckets" style={{ fontSize: 13, color: "var(--dim)" }}>
            Buckets
          </Link>
          <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
          <span
            style={{
              fontSize: 13,
              fontWeight: 500,
              color: "var(--tx)",
              ...monoStyle,
            }}
          >
            {name}
          </span>
        </div>
      }
      right={
        <HeaderSearch
          value={prefixQuery}
          onChange={(v) => {
            setPrefixQuery(v);
            setSel([]);
          }}
          placeholder="Lọc prefix…"
        />
      }
    />
  );

  if (!bucket) {
    return (
      <>
        {header}
        <Page>
          <TableWrap>
            <TableEmpty
              title="Không tìm thấy bucket"
              text={`Bucket "${name}" không tồn tại trong tài khoản của bạn.`}
              action={
                <Link to="/buckets" style={{ fontSize: 13, fontWeight: 500 }}>
                  Về danh sách bucket
                </Link>
              }
            />
          </TableWrap>
        </Page>
      </>
    );
  }

  const q = quotaView(bucket.used, bucket.max, bucket.res);
  const rows = rowsFor(activePrefix).filter((r) => !deleted.includes(r.key));

  const crumbs = [
    { label: bucket.name, color: "var(--acc)", to: "" },
    ...activePrefix
      .split("/")
      .filter(Boolean)
      .map((part, i, arr) => ({
        label: part,
        color: i === arr.length - 1 ? "var(--tx)" : "var(--dim)",
        to: `${arr.slice(0, i + 1).join("/")}/`,
      })),
  ];

  function goto(to: string) {
    setPrefix(to);
    setPrefixQuery("");
    setSel([]);
  }

  async function copyKey(key: string) {
    try {
      await navigator.clipboard?.writeText(key);
    } catch {
      // clipboard unavailable — still confirm to the user
    }
    setCopied(key);
    toast("Đã copy vào clipboard");
    setTimeout(() => setCopied(null), 2600);
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
            marginBottom: 16,
          }}
        >
          <div>
            <h1
              style={{
                fontSize: 22,
                fontWeight: 600,
                letterSpacing: "-.02em",
                margin: 0,
                ...monoStyle,
              }}
            >
              {bucket.name}
            </h1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 6 }}>
              {q.unlimited ? `${fmt(bucket.used)} · Không giới hạn` : q.pctText}{" "}
              · {grp(bucket.objects)} object · {REGION}
            </div>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <Link
              to="/buckets/$name/settings"
              params={{ name: bucket.name }}
              className="btnGhost"
              style={{
                height: 36,
                padding: "0 14px",
                border: "1px solid var(--line2)",
                background: "var(--panel)",
                color: "var(--tx)",
                borderRadius: 8,
                fontSize: 13,
                fontWeight: 500,
                display: "grid",
                placeItems: "center",
              }}
            >
              Cài đặt bucket
            </Link>
            <button
              type="button"
              disabled
              title="Sắp có — chờ S3 API slice #3"
              style={{
                height: 36,
                padding: "0 16px",
                border: "1px solid var(--line)",
                background: "var(--panel2)",
                color: "var(--faint)",
                borderRadius: 8,
                fontSize: 13,
                fontWeight: 600,
                cursor: "not-allowed",
              }}
            >
              Upload
            </button>
          </div>
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            marginBottom: 14,
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              flexWrap: "wrap",
              gap: 2,
              background: "var(--panel)",
              border: "1px solid var(--line)",
              borderRadius: 9,
              padding: "6px 10px",
              flex: 1,
              minHeight: 36,
            }}
          >
            {crumbs.map((c) => (
              <span
                key={c.to || "root"}
                style={{ display: "flex", alignItems: "center" }}
              >
                <button
                  type="button"
                  onClick={() => goto(c.to)}
                  style={{
                    background: "none",
                    border: 0,
                    padding: "2px 4px",
                    cursor: "pointer",
                    ...monoStyle,
                    fontSize: 12.5,
                    color: c.color,
                  }}
                >
                  {c.label}
                </button>
                <span style={{ color: "var(--faint)", fontSize: 12.5 }}>/</span>
              </span>
            ))}
          </div>
        </div>

        <div
          style={{
            background: "var(--panel)",
            border: "1px solid var(--line)",
            borderRadius: 12,
            overflow: "hidden",
          }}
        >
          {rows.length === 0 ? (
            <div style={{ padding: "56px 24px", textAlign: "center" }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>
                Bucket này chưa có object
              </div>
              <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 6 }}>
                Đẩy file đầu tiên bằng aws-cli:
              </div>
              <pre
                style={{
                  display: "inline-block",
                  margin: "16px 0 0",
                  padding: "12px 16px",
                  background: "#0C0B0A",
                  border: "1px solid var(--line)",
                  borderRadius: 9,
                  ...monoStyle,
                  fontSize: 12.5,
                  color: "#D8D2C8",
                  textAlign: "left",
                }}
              >
                {`aws s3 cp ./file.png s3://${bucket.name}/ \\\n  --endpoint-url ${ENDPOINT}`}
              </pre>
            </div>
          ) : (
            <table style={{ width: "100%", borderCollapse: "collapse" }}>
              <thead>
                <tr>
                  <Th width={44} />
                  <Th>KEY</Th>
                  <Th align="right" width={110}>
                    KÍCH THƯỚC
                  </Th>
                  <Th width={150}>LOẠI</Th>
                  <Th width={130}>ETAG</Th>
                  <Th width={150}>SỬA LÚC</Th>
                  <Th width={56} />
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr
                    key={r.key}
                    className="trHover"
                    style={{ borderBottom: "1px solid var(--line)" }}
                  >
                    <Td align="center" style={{ padding: 0 }}>
                      {r.kind === "file" && (
                        <input
                          type="checkbox"
                          checked={sel.includes(r.key)}
                          onChange={() =>
                            setSel(
                              sel.includes(r.key)
                                ? sel.filter((k) => k !== r.key)
                                : [...sel, r.key],
                            )
                          }
                          aria-label="Chọn object"
                          style={{
                            accentColor: "var(--acc)",
                            width: 14,
                            height: 14,
                            cursor: "pointer",
                          }}
                        />
                      )}
                    </Td>
                    <Td>
                      <button
                        type="button"
                        onClick={() => r.kind === "dir" && goto(r.key)}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 9,
                          background: "none",
                          border: 0,
                          padding: 0,
                          cursor: r.kind === "dir" ? "pointer" : "default",
                          ...monoStyle,
                          fontSize: "var(--fs)",
                          color: r.kind === "dir" ? "var(--tx)" : "var(--dim)",
                        }}
                      >
                        <span
                          style={{
                            color:
                              r.kind === "dir" ? "var(--acc)" : "var(--faint)",
                            fontSize: 13,
                          }}
                        >
                          {r.kind === "dir" ? "▸" : "·"}
                        </span>
                        {r.label}
                      </button>
                    </Td>
                    <Td
                      align="right"
                      style={{
                        ...monoStyle,
                        fontSize: 13,
                        color: "var(--dim)",
                      }}
                    >
                      {r.size}
                    </Td>
                    <Td>
                      {r.kind === "file" && (
                        <span
                          style={{
                            ...monoStyle,
                            fontSize: 11,
                            color: "var(--dim)",
                            background: "var(--panel2)",
                            border: "1px solid var(--line2)",
                            borderRadius: 5,
                            padding: "3px 7px",
                          }}
                        >
                          {r.type}
                        </span>
                      )}
                    </Td>
                    <Td
                      style={{
                        ...monoStyle,
                        fontSize: 12.5,
                        color: "var(--faint)",
                      }}
                    >
                      {r.etag}
                    </Td>
                    <Td style={{ fontSize: 13, color: "var(--dim)" }}>
                      {r.updated}
                    </Td>
                    <Td align="center" style={{ padding: "0 8px" }}>
                      {r.kind === "file" && (
                        <button
                          type="button"
                          className="iconBtn"
                          onClick={() => copyKey(r.key)}
                          aria-label="Copy key"
                          style={{
                            width: 28,
                            height: 28,
                            border: 0,
                            background: "none",
                            color: "var(--dim)",
                            borderRadius: 6,
                            cursor: "pointer",
                            fontSize: 13,
                          }}
                        >
                          {copied === r.key ? "✓" : "⧉"}
                        </button>
                      )}
                    </Td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        {sel.length > 0 && (
          <div
            style={{
              position: "sticky",
              bottom: 20,
              marginTop: 16,
              display: "flex",
              justifyContent: "center",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 16,
                background: "var(--panel2)",
                border: "1px solid var(--line2)",
                boxShadow: "0 16px 40px rgba(0,0,0,.55)",
                borderRadius: 11,
                padding: "10px 14px",
              }}
            >
              <span style={{ fontSize: 13, color: "var(--dim)" }}>
                {sel.length} object đã chọn
              </span>
              <button
                type="button"
                onClick={() => setSel([])}
                style={{
                  height: 30,
                  padding: "0 12px",
                  border: "1px solid var(--line2)",
                  background: "var(--panel)",
                  color: "var(--dim)",
                  borderRadius: 7,
                  fontSize: 12.5,
                  cursor: "pointer",
                }}
              >
                Bỏ chọn
              </button>
              <button
                type="button"
                onClick={() => {
                  // TODO(slice#7): DELETE /api/buckets/:pid/objects {keys}
                  setDeleted([...deleted, ...sel]);
                  toast(`Đã xoá ${sel.length} object`);
                  setSel([]);
                }}
                style={{
                  height: 30,
                  padding: "0 12px",
                  border: 0,
                  background: "var(--dgr)",
                  color: "#1A0D03",
                  borderRadius: 7,
                  fontSize: 12.5,
                  fontWeight: 600,
                  cursor: "pointer",
                }}
              >
                Xoá {sel.length} object
              </button>
            </div>
          </div>
        )}
      </Page>
    </>
  );
}
