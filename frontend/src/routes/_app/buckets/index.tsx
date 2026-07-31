// Ported from console-object-storage-gate/project/Buckets.dc.html.
// TODO(slice#7): the prototype's loading skeleton (lines 63-77) and quota-error
// banner need GET /api/buckets to drive them.
import { Link, createFileRoute } from "@tanstack/react-router";
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
import { validateBucketName } from "../../../lib/bucket-name";
import { fmt, grp, quotaView } from "../../../lib/format";
import { BUCKETS, type Bucket, UNITS } from "../../../lib/mock";

export const Route = createFileRoute("/_app/buckets/")({ component: Buckets });

type NewBucket = {
  name: string;
  num: string;
  unit: keyof typeof UNITS;
  unlimited: boolean;
};

const EMPTY_FORM: NewBucket = {
  name: "",
  num: "50",
  unit: "GiB",
  unlimited: false,
};

function Buckets() {
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const menu = useRowMenu();

  const [buckets, setBuckets] = useState<Bucket[]>(BUCKETS);
  const [query, setQuery] = useState("");
  const [form, setForm] = useState<NewBucket | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

  const rows = query
    ? buckets.filter((b) =>
        b.name.toLowerCase().includes(query.trim().toLowerCase()),
      )
    : buckets;

  const totalUsed = buckets.reduce((a, b) => a + b.used, 0);
  const nameError = form
    ? validateBucketName(
        form.name,
        buckets.map((b) => b.name),
      )
    : "";
  const nameValid = !!form && form.name.length > 0 && !nameError;

  function createBucket() {
    if (!form || !nameValid) return;
    const max = form.unlimited
      ? 0
      : Number.parseFloat(form.num || "0") * UNITS[form.unit];
    // TODO(slice#7): POST /api/buckets {name, max_bytes}
    setBuckets([
      ...buckets,
      {
        name: form.name,
        used: 0,
        max,
        res: 0,
        objects: 0,
        created: "vừa xong",
        full: "vừa xong",
      },
    ]);
    setForm(null);
    toast(`Đã tạo bucket ${form.name}`);
  }

  function deleteBucket(name: string) {
    // TODO(slice#7): DELETE /api/buckets/:pid
    setBuckets(buckets.filter((b) => b.name !== name));
    setDeleting(null);
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
          <PageAction
            label="Tạo bucket"
            onClick={() => setForm({ ...EMPTY_FORM })}
          />
        </div>

        <TableWrap>
          {buckets.length === 0 ? (
            <TableEmpty
              title="Chưa có bucket nào"
              text="Tạo bucket đầu tiên để bắt đầu đẩy object qua gateway."
              action={
                <PageAction
                  label="Tạo bucket"
                  onClick={() => setForm({ ...EMPTY_FORM })}
                />
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
                    const q = quotaView(b.used, b.max, b.res);
                    const id = `b-${b.name}`;
                    return (
                      <tr
                        key={b.name}
                        className="trHover"
                        style={{ borderBottom: "1px solid var(--line)" }}
                      >
                        <Td>
                          <Link
                            to="/buckets/$name"
                            params={{ name: b.name }}
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
                                ? `${fmt(b.used)} đã dùng · ∞`
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
                          {grp(b.objects)}
                        </Td>
                        <Td
                          title={b.full}
                          style={{ fontSize: 13, color: "var(--dim)" }}
                        >
                          {b.created}
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
                                to="/buckets/$name"
                                params={{ name: b.name }}
                                className="menuItem"
                                style={menuItemStyle}
                                onClick={menu.close}
                              >
                                Mở object browser
                              </Link>
                              <Link
                                to="/buckets/$name/settings"
                                params={{ name: b.name }}
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
                                  setDeleting(b.name);
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
              enabled={nameValid}
              onClick={createBucket}
            />
          </FormFoot>
        </FormModal>
      )}

      {deleting && (
        <ConfirmDangerModal
          title={`Xoá bucket ${deleting}`}
          body="Hành động này xoá cascade toàn bộ metadata object trong bucket. Không hoàn tác được."
          target={deleting}
          confirmLabel="Xoá bucket"
          onClose={() => setDeleting(null)}
          onConfirm={() => deleteBucket(deleting)}
        />
      )}
    </>
  );
}
