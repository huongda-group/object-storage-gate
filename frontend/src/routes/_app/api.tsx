// API page: the account token, the endpoint list, and a live connection check.
// The endpoints below are the same ones this console calls with its JWT — a PAT
// just lets a service reach them without a login. S3 client snippets are
// deliberately absent until SigV4 lands — see
// docs/superpowers/specs/2026-07-30-management-api-design.md §6.
import { createFileRoute } from "@tanstack/react-router";
import { Fragment, useEffect, useState } from "react";
import { Header } from "../../components/Header";
import { ConfirmDangerModal } from "../../components/Modal";
import { useToast } from "../../components/Toast";
import { useShell } from "../../components/shell";
import { H1, Page, TableWrap, Td, Th, monoStyle } from "../../components/ui";
import { getPat, rotatePat, whoami } from "../../lib/keys";

export const Route = createFileRoute("/_app/api")({ component: ApiPage });

const ENDPOINTS: { method: string; path: string; desc: string }[] = [
  {
    method: "GET",
    path: "/api/whoami",
    desc: "Kiểm tra token, trả về tài khoản",
  },
  { method: "GET", path: "/api/keys", desc: "Danh sách access key" },
  {
    method: "POST",
    path: "/api/keys",
    desc: "Tạo access key (trả secret một lần)",
  },
  { method: "GET", path: "/api/keys/{pid}", desc: "Chi tiết một key" },
  {
    method: "PATCH",
    path: "/api/keys/{pid}",
    desc: "Sửa nhãn, quyền, prefix, trạng thái",
  },
  {
    method: "POST",
    path: "/api/keys/{pid}/rotate",
    desc: "Xoay khoá, key cũ chuyển disabled",
  },
  { method: "DELETE", path: "/api/keys/{pid}", desc: "Thu hồi vĩnh viễn" },
  {
    method: "GET",
    path: "/api/buckets",
    desc: "Danh sách bucket của tài khoản",
  },
  {
    method: "GET",
    path: "/api/usage",
    desc: "Dung lượng đã dùng và hạn mức",
  },
];

function curlFor(method: string, path: string, token: string): string {
  const t = token || "<TOKEN>";
  const base = `curl -X ${method} "$OSG_HOST${path}" \\\n  -H "Authorization: Bearer ${t}"`;
  return method === "POST" || method === "PATCH"
    ? `${base} \\\n  -H "Content-Type: application/json" \\\n  -d '{"label":"ci","permissions":["read","list"]}'`
    : base;
}

function ApiPage() {
  const { user, requestLogout } = useShell();
  const toast = useToast();

  const [token, setToken] = useState("");
  const [shown, setShown] = useState(false);
  const [rotating, setRotating] = useState(false);
  const [check, setCheck] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    getPat()
      .then((r) => setToken(r.token))
      .catch(() => setCheck("Không lấy được token"));
  }, []);

  async function copy(text: string) {
    try {
      await navigator.clipboard?.writeText(text);
    } catch {
      // clipboard unavailable — still confirm to the user
    }
    toast("Đã copy vào clipboard");
  }

  async function doRotate() {
    const r = await rotatePat();
    setToken(r.token);
    setShown(true);
    setRotating(false);
    toast("Đã đổi token — cập nhật config các service ngay");
  }

  async function runCheck() {
    const r = await whoami(token);
    setCheck(`HTTP ${r.status}\n${r.body}`);
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            API
          </div>
        }
      />
      <Page>
        <H1>API</H1>
        <div
          style={{ fontSize: 13, color: "var(--dim)", margin: "5px 0 18px" }}
        >
          Service khác gọi API này bằng token dưới đây. Mỗi tài khoản chỉ có một
          token — đổi token là mọi service đang dùng phải cập nhật cùng lúc.
        </div>

        <section style={{ marginBottom: 26 }}>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <code
              style={{
                ...monoStyle,
                flex: 1,
                padding: "10px 12px",
                borderRadius: 8,
                border: "1px solid var(--line2)",
                background: "var(--panel2)",
                fontSize: 13,
                overflowX: "auto",
              }}
            >
              {shown ? token : "•".repeat(Math.max(token.length, 24))}
            </code>
            <button
              type="button"
              className="btnGhost"
              onClick={() => setShown(!shown)}
            >
              {shown ? "Ẩn" : "Hiện"}
            </button>
            <button
              type="button"
              className="btnGhost"
              onClick={() => void copy(token)}
            >
              Copy
            </button>
            <button
              type="button"
              className="btnGhost"
              onClick={() => setRotating(true)}
            >
              Đổi token
            </button>
          </div>
          <div
            style={{
              marginTop: 10,
              display: "flex",
              gap: 8,
              alignItems: "center",
            }}
          >
            <button
              type="button"
              className="btnGhost"
              onClick={() => void runCheck()}
            >
              Kiểm tra kết nối
            </button>
            {check && (
              <pre
                style={{
                  ...monoStyle,
                  fontSize: 12,
                  color: "var(--dim)",
                  margin: 0,
                }}
              >
                {check}
              </pre>
            )}
          </div>
        </section>

        <TableWrap>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                <Th width={80}>METHOD</Th>
                <Th width={260}>PATH</Th>
                <Th>MÔ TẢ</Th>
                <Th width={90} />
              </tr>
            </thead>
            <tbody>
              {ENDPOINTS.map((e) => {
                const id = `${e.method} ${e.path}`;
                return (
                  <Fragment key={id}>
                    <tr style={{ borderBottom: "1px solid var(--line)" }}>
                      <Td style={{ ...monoStyle, fontSize: 12.5 }}>
                        {e.method}
                      </Td>
                      <Td style={{ ...monoStyle, fontSize: 12.5 }}>{e.path}</Td>
                      <Td style={{ fontSize: 12.5, color: "var(--dim)" }}>
                        {e.desc}
                      </Td>
                      <Td align="center">
                        <button
                          type="button"
                          className="btnGhost"
                          onClick={() => setOpen(open === id ? null : id)}
                        >
                          curl
                        </button>
                      </Td>
                    </tr>
                    {open === id && (
                      <tr>
                        <td
                          colSpan={4}
                          style={{
                            padding: "10px 12px",
                            background: "var(--panel2)",
                          }}
                        >
                          <pre
                            style={{
                              ...monoStyle,
                              fontSize: 12,
                              margin: 0,
                              whiteSpace: "pre-wrap",
                            }}
                          >
                            {curlFor(e.method, e.path, shown ? token : "")}
                          </pre>
                          <button
                            type="button"
                            className="btnGhost"
                            onClick={() =>
                              void copy(curlFor(e.method, e.path, token))
                            }
                          >
                            Copy lệnh
                          </button>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </TableWrap>
      </Page>

      {rotating && (
        <ConfirmDangerModal
          title="Đổi token quản trị"
          body="Token cũ mất hiệu lực ngay. Mọi service đang dùng token này sẽ nhận 401 cho tới khi cập nhật config."
          target={user.email}
          confirmLabel="Đổi token"
          onClose={() => setRotating(false)}
          onConfirm={() => void doRotate()}
        />
      )}
    </>
  );
}
