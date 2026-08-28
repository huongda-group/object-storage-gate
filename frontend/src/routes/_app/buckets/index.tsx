// Ported from console-object-storage-gate/project/Buckets.dc.html.
// TODO(slice#7): the prototype's loading skeleton (lines 63-77) and quota-error banner need GET /api/buckets to drive them.
import { Link, createFileRoute, useRouter } from "@tanstack/react-router";
import { useState } from "react";
import { Header } from "../../../components/Header";
import {
  ConfirmDangerModal,
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
  HeaderSearch,
  Page,
  PageAction,
  QuotaBar,
  QuotaFields,
  RowMenu,
  RowMenuButton,
  TableEmpty,
  TableFoot,
  TableWrap,
  Td,
  Th,
  menuItemStyle,
  monoStyle,
  useRowMenu,
} from "../../../components/ui";
import { run } from "../../../lib/api-client";
import { validateBucketName } from "../../../lib/bucket-name";
import {
  type Bucket,
  createBucket as apiCreateBucket,
  deleteBucket as apiDeleteBucket,
  listBuckets,
} from "../../../lib/buckets";
import { UNITS } from "../../../lib/dashboard";
import { fmt, grp, quotaView } from "../../../lib/format";
import { type PoolChoice, listPoolChoices } from "../../../lib/pools";

export const Route = createFileRoute("/_app/buckets/")({
  // A bucket cannot be created without a pool, so the form needs both lists before it renders.
  loader: async () => {
    const [buckets, pools] = await Promise.all([
      listBuckets(),
      listPoolChoices(),
    ]);
    return { buckets, pools };
  },
  component: Buckets,
});

type NewBucket = {
  name: string;
  num: string;
  unit: keyof typeof UNITS;
  unlimited: boolean;
  poolPid: string;
};

const EMPTY_FORM: NewBucket = {
  name: "",
  num: "50",
  unit: "GiB",
  unlimited: false,
  poolPid: "",
};

function Buckets() {
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const menu = useRowMenu();

  const router = useRouter();
  const { buckets, pools }: { buckets: Bucket[]; pools: PoolChoice[] } =
    Route.useLoaderData();
  const canCreate = pools.length > 0;

  // One pool means there is nothing to choose; preselect it and leave the select out.
  const newForm = (): NewBucket => ({
    ...EMPTY_FORM,
    poolPid: pools.length === 1 ? pools[0].pid : "",
  });
  const [query, setQuery] = useState("");
  const [form, setForm] = useState<NewBucket | null>(null);
  const [deleting, setDeleting] = useState<Bucket | null>(null);
  const [busy, setBusy] = useState(false);

  const rows = query
    ? buckets.filter((b) =>
        b.name.toLowerCase().includes(query.trim().toLowerCase()),
      )
    : buckets;

  const totalUsed = buckets.reduce((a, b) => a + b.used_bytes, 0);
  const nameError = form
    ? validateBucketName(
        form.name,
        buckets.map((b) => b.name),
      )
    : "";
  const nameValid = !!form && form.name.length > 0 && !nameError;
  const formValid = nameValid && !!form?.poolPid;

  async function createBucket() {
    if (!form || !formValid || busy) return;
    setBusy(true);
    const max = form.unlimited
      ? 0
      : Math.round(Number.parseFloat(form.num || "0") * UNITS[form.unit]);
    const created = await run(
      () => apiCreateBucket(form.name, max, form.poolPid),
      { onError: (m) => toast(m, "danger") },
    );
    setBusy(false);
    if (!created) return;
    setForm(null);
    await router.invalidate();
    toast(`Đã tạo bucket ${created.name}`);
  }

  async function deleteBucket(pid: string, name: string) {
    if (busy) return;
    setBusy(true);
    const ok = await run(() => apiDeleteBucket(pid), {
      onError: (m) => toast(m, "danger"),
    });
    setBusy(false);
    setDeleting(null);
    if (ok === undefined) return;
    await router.invalidate();
    toast(`Đã xoá bucket ${name}`, "danger");
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            Buckets
          </div>
        }
        right={
          <HeaderSearch
            value={query}
            onChange={setQuery}
            placeholder="Tìm bucket…"
          />
        }
      />
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
            <H1>Buckets</H1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              {rows.length} bucket · {fmt(totalUsed)} tổng dung lượng
            </div>
          </div>
          {canCreate ? (
            <PageAction label="Tạo bucket" onClick={() => setForm(newForm())} />
          ) : (
            <div
              style={{
                fontSize: 12.5,
                color: "var(--dim)",
                textAlign: "right",
                maxWidth: 280,
                lineHeight: 1.5,
              }}
            >
              Chưa có pool nào. Liên hệ quản trị viên trước khi tạo bucket.
            </div>
          )}
        </div>

        <TableWrap>
          {buckets.length === 0 ? (
            <TableEmpty
              title="Chưa có bucket nào"
              text="Tạo bucket đầu tiên để bắt đầu đẩy object qua gateway."
              action={
                canCreate ? (
                  <PageAction
                    label="Tạo bucket"
                    onClick={() => setForm(newForm())}
                  />
                ) : undefined
              }
            />
          ) : (
            <>
              <table
                data-tmin=""
                style={{ width: "100%", borderCollapse: "collapse" }}
              >
                <thead>
                  <tr>
                    <Th>TÊN</Th>
                    <Th width={280}>DUNG LƯỢNG</Th>
                    <Th align="right" width={130}>
                      OBJECT
                    </Th>
                    <Th width={150}>TẠO LÚC</Th>
                    <Th width={56} />
                  </tr>
                </thead>
                <tbody>
                  {rows.map((b) => {
                    const q = quotaView(
                      b.used_bytes,
                      b.max_bytes,
                      b.reserved_bytes,
                    );
                    const id = `b-${b.name}`;
                    return (
                      <tr
                        key={b.name}
                        className="trHover"
                        style={{ borderBottom: "1px solid var(--line)" }}
                      >
                        <Td>
                          <Link
                            to="/buckets/$pid"
                            params={{ pid: b.pid }}
                            style={{
                              ...monoStyle,
                              fontSize: "var(--fs)",
                              color: "var(--acc)",
                              fontWeight: 500,
                            }}
                          >
                            {b.name}
                          </Link>
                        </Td>
                        <Td>
                          <div
                            style={{
                              display: "flex",
                              alignItems: "center",
                              gap: 12,
                            }}
                          >
                            {q.unlimited ? (
                              <div style={{ flex: 1, height: 5 }} />
                            ) : (
                              <div style={{ flex: 1 }}>
                                <QuotaBar q={q} height={5} />
                              </div>
                            )}
                            <div
                              style={{
                                width: 172,
                                fontSize: 12.5,
                                color: "var(--dim)",
                                ...monoStyle,
                                textAlign: "right",
                                whiteSpace: "nowrap",
                              }}
                            >
                              {q.unlimited
                                ? `${fmt(b.used_bytes)} đã dùng · ∞`
                                : q.usedLine}
                            </div>
                          </div>
                        </Td>
                        <Td
                          align="right"
                          style={{
                            ...monoStyle,
                            fontSize: 13,
                            color: "var(--dim)",
                          }}
                        >
                          {grp(b.object_count)}
                        </Td>
                        <Td
                          title={new Date(b.created_at).toLocaleString("vi-VN")}
                          style={{ fontSize: 13, color: "var(--dim)" }}
                        >
                          {new Date(b.created_at).toLocaleDateString("vi-VN")}
                        </Td>
                        <Td
                          align="center"
                          style={{ padding: "0 8px", position: "relative" }}
                        >
                          <RowMenuButton
                            onClick={(e) => menu.toggle(id, e, 126)}
                          />
                          {menu.open === id && (
                            <RowMenu pos={menu.pos}>
                              <Link
                                to="/buckets/$pid"
                                params={{ pid: b.pid }}
                                className="menuItem"
                                style={menuItemStyle}
                                onClick={menu.close}
                              >
                                Mở object browser
                              </Link>
                              <Link
                                to="/buckets/$pid/settings"
                                params={{ pid: b.pid }}
                                className="menuItem"
                                style={menuItemStyle}
                                onClick={menu.close}
                              >
                                Sửa quota
                              </Link>
                              <button
                                type="button"
                                className="menuItemDanger"
                                style={{
                                  ...menuItemStyle,
                                  color: "var(--dgr)",
                                }}
                                onClick={() => {
                                  menu.close();
                                  setDeleting(b);
                                }}
                              >
                                Xoá bucket
                              </button>
                            </RowMenu>
                          )}
                        </Td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              <TableFoot shown={rows.length} total={buckets.length} />
            </>
          )}
        </TableWrap>
      </Page>

      {form && (
        <FormModal onClose={() => setForm(null)}>
          <FormHead
            title="Tạo bucket"
            sub="Tên bucket phải hợp lệ theo luật S3 và chưa tồn tại trong tài khoản của bạn."
          />
          <FormBody>
            <label
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 7,
                fontSize: 12,
                fontWeight: 500,
                color: "var(--dim)",
              }}
            >
              Tên bucket
              <input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder="media-cdn"
                style={{
                  height: 38,
                  borderRadius: 8,
                  border: `1px solid ${
                    nameError
                      ? "var(--dgr)"
                      : nameValid
                        ? "var(--ok)"
                        : "var(--line2)"
                  }`,
                  background: "var(--panel2)",
                  color: "var(--tx)",
                  padding: "0 12px",
                  fontSize: 14,
                  ...monoStyle,
                }}
              />
            </label>
            {nameError && (
              <div style={{ marginTop: -8, fontSize: 12.5, color: "#FF9AA2" }}>
                {nameError}
              </div>
            )}
            {nameValid && (
              <div
                style={{ marginTop: -8, fontSize: 12.5, color: "var(--ok)" }}
              >
                Tên hợp lệ
              </div>
            )}

            {pools.length > 1 && (
              <div>
                <div
                  style={{
                    fontSize: 12,
                    fontWeight: 500,
                    color: "var(--dim)",
                    marginBottom: 7,
                  }}
                >
                  Pool
                </div>
                <select
                  value={form.poolPid}
                  onChange={(e) =>
                    setForm({ ...form, poolPid: e.target.value })
                  }
                  style={{
                    width: "100%",
                    height: 38,
                    borderRadius: 8,
                    border: "1px solid var(--line2)",
                    background: "var(--panel2)",
                    color: "var(--tx)",
                    padding: "0 10px",
                    fontSize: 13.5,
                  }}
                >
                  <option value="">Chọn pool…</option>
                  {pools.map((p) => (
                    <option key={p.pid} value={p.pid}>
                      {p.name} ({p.provider})
                    </option>
                  ))}
                </select>
              </div>
            )}

            <div>
              <div
                style={{
                  fontSize: 12,
                  fontWeight: 500,
                  color: "var(--dim)",
                  marginBottom: 7,
                }}
              >
                Quota bucket
              </div>
              <QuotaFields
                num={form.num}
                unit={form.unit}
                unlimited={form.unlimited}
                onNum={(num) => setForm({ ...form, num })}
                onUnit={(unit) => setForm({ ...form, unit })}
                onUnlimited={(unlimited) => setForm({ ...form, unlimited })}
              />
            </div>
          </FormBody>
          <FormFoot>
            <FormCancel onClick={() => setForm(null)} />
            <FormSubmit
              label="Tạo bucket"
              enabled={formValid}
              onClick={createBucket}
            />
          </FormFoot>
        </FormModal>
      )}

      {deleting && (
        <ConfirmDangerModal
          title={`Xoá bucket ${deleting.name}`}
          body="Chỉ xoá được bucket rỗng: server từ chối nếu còn object, vì metadata mồ côi sẽ không khớp với store thật."
          target={deleting.name}
          confirmLabel="Xoá bucket"
          onClose={() => setDeleting(null)}
          onConfirm={() => void deleteBucket(deleting.pid, deleting.name)}
        />
      )}
    </>
  );
}
