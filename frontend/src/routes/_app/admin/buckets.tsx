import { createFileRoute, redirect, useRouter } from "@tanstack/react-router";
import { useState } from "react";
import { Header } from "../../../components/Header";
import {
  FormBody,
  FormCancel,
  FormFoot,
  FormHead,
  FormModal,
  FormSubmit,
} from "../../../components/Modal";
import { useToast } from "../../../components/Toast";
import { useShell } from "../../../components/shell";
import {
  H1,
  Page,
  RowMenu,
  RowMenuButton,
  TableWrap,
  Td,
  Th,
  menuItemStyle,
  monoStyle,
  useRowMenu,
} from "../../../components/ui";
import { run } from "../../../lib/api-client";
import {
  PROVIDERS,
  type Pool,
  type Provider,
  createPool,
  deletePool,
  listPools,
  updatePool,
} from "../../../lib/pools";

export const Route = createFileRoute("/_app/admin/buckets")({
  // UX guard only — AdminCaller is the real gate, on the server.
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  loader: () => listPools(),
  component: AdminPools,
});

type PoolForm = {
  mode: "create" | "edit";
  pid: string | null;
  name: string;
  provider: Provider;
  region: string;
  api_endpoint: string;
  physical_bucket: string;
  access_id: string;
  access_secret: string;
  touched: boolean;
};

const NEW_POOL: PoolForm = {
  mode: "create",
  pid: null,
  name: "",
  provider: "aws",
  region: "",
  api_endpoint: "",
  physical_bucket: "",
  access_id: "",
  access_secret: "",
  touched: false,
};

const field = {
  width: "100%",
  height: 38,
  borderRadius: 8,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--tx)",
  padding: "0 12px",
  fontSize: 13.5,
} as const;

const label = { fontSize: 12, color: "var(--dim)", marginBottom: 6 } as const;

const hint = {
  fontSize: 12,
  color: "var(--faint)",
  marginTop: 5,
  lineHeight: 1.5,
} as const;

function AdminPools() {
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const menu = useRowMenu();
  const router = useRouter();

  const pools: Pool[] = Route.useLoaderData();
  const [busy, setBusy] = useState(false);
  const [form, setForm] = useState<PoolForm | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Pool | null>(null);

  const nameTrim = form?.name.trim() ?? "";
  const nameDup =
    form?.mode === "create" && pools.some((p) => p.name === nameTrim);
  const nameErr = !nameTrim
    ? "Tên pool không được để trống."
    : nameDup
      ? "Tên pool đã tồn tại."
      : "";
  const bucketErr = form?.physical_bucket.trim()
    ? ""
    : "Physical bucket không được để trống.";
  const formValid = !nameErr && !bucketErr;

  async function savePool() {
    if (!form || busy) return;
    if (!formValid) {
      setForm({ ...form, touched: true });
      return;
    }
    setBusy(true);

    const saved =
      form.mode === "create"
        ? await run(
            () =>
              createPool({
                name: nameTrim,
                provider: form.provider,
                region: form.region.trim() || undefined,
                api_endpoint: form.api_endpoint.trim() || undefined,
                physical_bucket: form.physical_bucket.trim(),
                access_id: form.access_id.trim() || undefined,
                access_secret: form.access_secret || undefined,
              }),
            { onError: (m) => toast(m, "danger") },
          )
        : await run(
            () =>
              // updatePool drops blank fields: an empty secret means unchanged, never erase.
              updatePool(form.pid ?? "", {
                region: form.region.trim(),
                api_endpoint: form.api_endpoint.trim(),
                physical_bucket: form.physical_bucket.trim(),
                access_id: form.access_id.trim(),
                access_secret: form.access_secret,
              }),
            { onError: (m) => toast(m, "danger") },
          );

    setBusy(false);
    if (!saved) return;
    setForm(null);
    await router.invalidate();
    toast(form.mode === "create" ? "Đã tạo pool" : "Đã cập nhật pool");
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            Pool
          </div>
        }
      />
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
            <H1>Pool</H1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              {pools.length} pool — mỗi bucket của user trỏ tới đúng một pool
            </div>
          </div>
          <button
            type="button"
            onClick={() => setForm({ ...NEW_POOL })}
            style={{
              height: 34,
              padding: "0 16px",
              border: 0,
              background: "var(--acc)",
              color: "var(--accTx)",
              borderRadius: 8,
              fontSize: 13,
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            + Tạo pool
          </button>
        </div>

        <TableWrap>
          <table
            data-tmin=""
            style={{ width: "100%", borderCollapse: "collapse" }}
          >
            <thead>
              <tr>
                <Th>TÊN</Th>
                <Th width={110}>PROVIDER</Th>
                <Th width={190}>PHYSICAL BUCKET</Th>
                <Th>ENDPOINT</Th>
                <Th width={190}>CREDENTIAL</Th>
                <Th width={56} />
              </tr>
            </thead>
            <tbody>
              {pools.map((p) => (
                <tr
                  key={p.pid}
                  className="trHover"
                  style={{ borderBottom: "1px solid var(--line)" }}
                >
                  <Td style={{ fontSize: "var(--fs)" }}>{p.name}</Td>
                  <Td style={{ fontSize: 13, color: "var(--dim)" }}>
                    {p.provider}
                  </Td>
                  <Td style={{ fontSize: 12.5, ...monoStyle }}>
                    {p.physical_bucket}
                  </Td>
                  <Td
                    style={{
                      fontSize: 12.5,
                      color: "var(--dim)",
                      ...monoStyle,
                    }}
                  >
                    {p.api_endpoint ?? "—"}
                  </Td>
                  <Td
                    style={{
                      fontSize: 12.5,
                      fontWeight: p.is_configured ? 400 : 600,
                      color: p.is_configured ? "var(--dim)" : "var(--dgr)",
                    }}
                  >
                    {p.is_configured
                      ? (p.access_id ?? "đã cấu hình")
                      : "CHƯA CÓ CREDENTIAL"}
                  </Td>
                  <Td
                    align="center"
                    style={{ padding: "0 8px", position: "relative" }}
                  >
                    <RowMenuButton
                      onClick={(e) => menu.toggle(`p-${p.pid}`, e, 168)}
                    />
                    {menu.open === `p-${p.pid}` && (
                      <RowMenu pos={menu.pos}>
                        <button
                          type="button"
                          className="menuItem"
                          style={menuItemStyle}
                          onClick={() => {
                            menu.close();
                            setForm({
                              mode: "edit",
                              pid: p.pid,
                              name: p.name,
                              provider: p.provider,
                              region: p.region ?? "",
                              api_endpoint: p.api_endpoint ?? "",
                              physical_bucket: p.physical_bucket,
                              access_id: p.access_id ?? "",
                              // Never prefilled: the server does not return it.
                              access_secret: "",
                              touched: false,
                            });
                          }}
                        >
                          Sửa cấu hình
                        </button>
                        <button
                          type="button"
                          className="menuItem"
                          style={{ ...menuItemStyle, color: "var(--dgr)" }}
                          onClick={() => {
                            menu.close();
                            setConfirmDelete(p);
                          }}
                        >
                          Xoá pool
                        </button>
                      </RowMenu>
                    )}
                  </Td>
                </tr>
              ))}
            </tbody>
          </table>
        </TableWrap>

        {pools.length === 0 && (
          <div
            style={{
              marginTop: 14,
              fontSize: 13,
              color: "var(--dim)",
              lineHeight: 1.6,
            }}
          >
            Chưa có pool nào. Gateway không phục vụ được request S3 nào cho tới
            khi có ít nhất một pool đã cấu hình credential.
          </div>
        )}
      </Page>

      {form && (
        <FormModal onClose={() => setForm(null)}>
          <FormHead
            title={form.mode === "create" ? "Tạo pool" : "Sửa cấu hình pool"}
            sub={
              form.mode === "create"
                ? "Một pool là object store thật cộng với physical bucket bên trong nó."
                : form.name
            }
          />
          <FormBody padding="18px 24px" gap={14}>
            <div>
              <div style={label}>Tên pool</div>
              <input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                disabled={form.mode === "edit"}
                placeholder="main"
                style={{
                  ...field,
                  border: `1px solid ${
                    form.touched && nameErr ? "var(--dgr)" : "var(--line2)"
                  }`,
                }}
              />
              {form.touched && nameErr && (
                <div style={{ ...hint, color: "var(--dgr)" }}>{nameErr}</div>
              )}
            </div>

            <div>
              <div style={label}>Provider</div>
              <select
                value={form.provider}
                disabled={form.mode === "edit"}
                onChange={(e) =>
                  setForm({ ...form, provider: e.target.value as Provider })
                }
                style={field}
              >
                {PROVIDERS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </div>

            <div>
              <div style={label}>Physical bucket</div>
              <input
                value={form.physical_bucket}
                onChange={(e) =>
                  setForm({ ...form, physical_bucket: e.target.value })
                }
                placeholder="osg-main"
                style={{
                  ...field,
                  fontFamily: "'IBM Plex Mono',monospace",
                  border: `1px solid ${
                    form.touched && bucketErr ? "var(--dgr)" : "var(--line2)"
                  }`,
                }}
              />
              <div
                style={{
                  ...hint,
                  color:
                    form.touched && bucketErr ? "var(--dgr)" : "var(--faint)",
                }}
              >
                {form.touched && bucketErr
                  ? bucketErr
                  : "Bucket thật trên object store. Client không bao giờ thấy tên này."}
              </div>
            </div>

            <div>
              <div style={label}>Region</div>
              <input
                value={form.region}
                onChange={(e) => setForm({ ...form, region: e.target.value })}
                placeholder="ap-southeast-1"
                style={field}
              />
            </div>

            <div>
              <div style={label}>API endpoint</div>
              <input
                value={form.api_endpoint}
                onChange={(e) =>
                  setForm({ ...form, api_endpoint: e.target.value })
                }
                placeholder="https://minio.internal:9000"
                style={{ ...field, fontFamily: "'IBM Plex Mono',monospace" }}
              />
              <div style={hint}>Để trống nếu dùng AWS S3.</div>
            </div>

            <div>
              <div style={label}>Access key ID</div>
              <input
                value={form.access_id}
                onChange={(e) =>
                  setForm({ ...form, access_id: e.target.value })
                }
                placeholder="AKIA…"
                style={{ ...field, fontFamily: "'IBM Plex Mono',monospace" }}
              />
            </div>

            <div>
              <div style={label}>Access key secret</div>
              <input
                type="password"
                value={form.access_secret}
                onChange={(e) =>
                  setForm({ ...form, access_secret: e.target.value })
                }
                placeholder={
                  form.mode === "edit"
                    ? "Để trống nếu không đổi"
                    : "Secret của upstream"
                }
                autoComplete="new-password"
                style={{ ...field, fontFamily: "'IBM Plex Mono',monospace" }}
              />
              {form.mode === "edit" && (
                <div style={hint}>
                  Để trống nếu không đổi. Máy chủ không trả secret về, nên không
                  có gì để prefill.
                </div>
              )}
            </div>
          </FormBody>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setForm(null)} />
            <FormSubmit
              label={form.mode === "create" ? "Tạo pool" : "Lưu thay đổi"}
              enabled
              onClick={() => void savePool()}
            />
          </FormFoot>
        </FormModal>
      )}

      {confirmDelete && (
        <FormModal width={440} onClose={() => setConfirmDelete(null)}>
          <FormHead
            title="Xoá pool"
            sub={`${confirmDelete.name} — ${confirmDelete.physical_bucket}`}
          />
          <div
            style={{
              padding: "18px 24px",
              fontSize: 13,
              color: "var(--dim)",
              lineHeight: 1.6,
            }}
          >
            Máy chủ từ chối nếu còn bucket nào trỏ tới pool này. Không có dữ
            liệu nào trên object store bị xoá.
          </div>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setConfirmDelete(null)} />
            <FormSubmit
              label="Xoá pool"
              enabled
              onClick={() =>
                void (async () => {
                  const done = await run(() => deletePool(confirmDelete.pid), {
                    onError: (m) => toast(m, "danger"),
                  });
                  setConfirmDelete(null);
                  if (done === undefined) return;
                  await router.invalidate();
                  toast("Đã xoá pool");
                })()
              }
            />
          </FormFoot>
        </FormModal>
      )}
    </>
  );
}
