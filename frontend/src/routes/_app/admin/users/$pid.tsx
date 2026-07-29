// Ported from console-object-storage-gate/project/Admin User Detail.dc.html.
// `$pid` is the user's email while the mock data has no pid (see users/index.tsx).
import { Link, createFileRoute, redirect } from "@tanstack/react-router";
import { useState } from "react";
import { Header } from "../../../../components/Header";
import {
  FormBody,
  FormCancel,
  FormFoot,
  FormHead,
  FormModal,
  FormSubmit,
} from "../../../../components/Modal";
import { useToast } from "../../../../components/Toast";
import { useShell } from "../../../../components/shell";
import {
  Chip,
  Page,
  PillDot,
  QuotaFields,
  TableEmpty,
  TableWrap,
  Td,
  Th,
  monoStyle,
} from "../../../../components/ui";
import { colorFor, fmt, grp, pill, shortId } from "../../../../lib/format";
import {
  ADMIN_USERS,
  type AdminUser,
  BUCKETS,
  KEYS,
  UNITS,
} from "../../../../lib/mock";

export const Route = createFileRoute("/_app/admin/users/$pid")({
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  component: AdminUserDetail,
});

type Tab = "buckets" | "keys" | "activity";

const TABS: { key: Tab; label: string }[] = [
  { key: "buckets", label: "Buckets" },
  { key: "keys", label: "Access keys" },
  { key: "activity", label: "Hoạt động" },
];

const actionBtn = {
  height: 34,
  padding: "0 14px",
  border: "1px solid var(--line2)",
  background: "var(--panel)",
  color: "var(--tx)",
  borderRadius: 8,
  fontSize: 13,
  fontWeight: 500,
  cursor: "pointer",
} as const;

const roleSelect = {
  width: "100%",
  height: 38,
  borderRadius: 8,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--tx)",
  padding: "0 10px",
  fontSize: 13.5,
} as const;

function AdminUserDetail() {
  const { pid } = Route.useParams();
  const { user, requestLogout } = useShell();
  const toast = useToast();

  const found = ADMIN_USERS.find((u) => u.email === pid);
  const [target, setTarget] = useState<AdminUser | undefined>(found);
  const [tab, setTab] = useState<Tab>("buckets");
  const [editName, setEditName] = useState<{
    name: string;
    touched: boolean;
  } | null>(null);
  const [quotaForm, setQuotaForm] = useState<{
    num: string;
    unit: keyof typeof UNITS;
    unlimited: boolean;
  } | null>(null);
  const [nextRole, setNextRole] = useState<"user" | "admin" | null>(null);

  const header = (
    <Header
      user={user}
      onLogout={requestLogout}
      left={
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Link to="/admin" style={{ fontSize: 13, color: "var(--dim)" }}>
            Admin
          </Link>
          <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
          <Link to="/admin/users" style={{ fontSize: 13, color: "var(--dim)" }}>
            Users
          </Link>
          <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            {pid}
          </span>
        </div>
      }
    />
  );

  if (!target) {
    return (
      <>
        {header}
        <Page>
          <TableWrap>
            <TableEmpty
              title="Không tìm thấy user"
              text={`Không có user nào với email "${pid}".`}
              action={
                <Link
                  to="/admin/users"
                  style={{ fontSize: 13, fontWeight: 500 }}
                >
                  Về danh sách user
                </Link>
              }
            />
          </TableWrap>
        </Page>
      </>
    );
  }

  const pct = target.max ? Math.min(100, (target.used / target.max) * 100) : 0;
  const quotaBytes = quotaForm
    ? quotaForm.unlimited
      ? 0
      : Number.parseFloat(quotaForm.num || "0") * UNITS[quotaForm.unit]
    : 0;
  const quotaWarn =
    !!quotaForm && !quotaForm.unlimited && quotaBytes < target.used;
  const selfDemote = target.email === user.email && nextRole === "user";

  return (
    <>
      {header}
      <Page>
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: 20,
            marginBottom: 20,
          }}
        >
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <h1
                style={{
                  fontSize: 20,
                  fontWeight: 600,
                  margin: 0,
                  letterSpacing: "-.01em",
                }}
              >
                {target.email}
              </h1>
              <span
                style={{
                  fontSize: 11,
                  fontWeight: 600,
                  padding: "3px 8px",
                  borderRadius: 5,
                  background:
                    target.role === "admin"
                      ? "var(--infoSoft)"
                      : "var(--panel2)",
                  color: target.role === "admin" ? "var(--info)" : "var(--dim)",
                }}
              >
                {target.role}
              </span>
            </div>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 6 }}>
              {target.name} · tạo {target.created}
            </div>
            <div style={{ width: 340, marginTop: 12 }}>
              <div
                style={{
                  height: 6,
                  borderRadius: 3,
                  background: "var(--line)",
                  overflow: "hidden",
                  display: "flex",
                }}
              >
                <div
                  style={{
                    width: `${pct.toFixed(1)}%`,
                    background: target.max ? colorFor(pct) : "var(--acc)",
                  }}
                />
              </div>
              <div
                style={{
                  fontSize: 12.5,
                  color: "var(--dim)",
                  marginTop: 6,
                  ...monoStyle,
                }}
              >
                {target.max
                  ? `${fmt(target.used)} / ${fmt(target.max)} (${pct.toFixed(1)}%)`
                  : `${fmt(target.used)} · Không giới hạn`}
              </div>
            </div>
          </div>
          <div style={{ display: "flex", gap: 8, flex: "0 0 auto" }}>
            <button
              type="button"
              className="btnGhost"
              onClick={() => setEditName({ name: target.name, touched: false })}
              style={actionBtn}
            >
              Sửa thông tin
            </button>
            <button
              type="button"
              className="btnGhost"
              onClick={() =>
                setQuotaForm({
                  num: "500",
                  unit: "GiB",
                  unlimited: !target.max,
                })
              }
              style={actionBtn}
            >
              Sửa quota
            </button>
            <button
              type="button"
              className="btnGhost"
              onClick={() => setNextRole(target.role)}
              style={actionBtn}
            >
              Đổi role
            </button>
          </div>
        </div>

        <div
          style={{
            display: "flex",
            gap: 4,
            borderBottom: "1px solid var(--line)",
            marginBottom: 16,
          }}
        >
          {TABS.map((t) => (
            <button
              key={t.key}
              type="button"
              onClick={() => setTab(t.key)}
              style={{
                height: 36,
                padding: "0 14px",
                border: 0,
                borderBottom: `2px solid ${tab === t.key ? "var(--acc)" : "transparent"}`,
                background: "none",
                color: tab === t.key ? "var(--tx)" : "var(--dim)",
                fontSize: 13,
                fontWeight: 500,
                cursor: "pointer",
              }}
            >
              {t.label}
            </button>
          ))}
        </div>

        {tab === "buckets" && (
          <TableWrap>
            <table
              data-tmin=""
              style={{ width: "100%", borderCollapse: "collapse" }}
            >
              <thead>
                <tr>
                  <Th>TÊN</Th>
                  <Th width={300}>DUNG LƯỢNG</Th>
                  <Th align="right" width={130}>
                    OBJECT
                  </Th>
                  <Th width={110} />
                </tr>
              </thead>
              <tbody>
                {BUCKETS.map((b) => {
                  const bp = b.max ? Math.min(100, (b.used / b.max) * 100) : 0;
                  return (
                    <tr
                      key={b.name}
                      className="trHover"
                      style={{ borderBottom: "1px solid var(--line)" }}
                    >
                      <Td style={{ ...monoStyle, fontSize: "var(--fs)" }}>
                        {b.name}
                      </Td>
                      <Td>
                        <div
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: 12,
                          }}
                        >
                          {b.max ? (
                            <div
                              style={{
                                flex: 1,
                                height: 5,
                                borderRadius: 3,
                                background: "var(--line)",
                                overflow: "hidden",
                                display: "flex",
                              }}
                            >
                              <div
                                style={{
                                  width: `${bp.toFixed(1)}%`,
                                  background: colorFor(bp),
                                }}
                              />
                            </div>
                          ) : (
                            <div style={{ flex: 1, height: 5 }} />
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
                            {b.max
                              ? `${fmt(b.used)} / ${fmt(b.max)}`
                              : `${fmt(b.used)} đã dùng · ∞`}
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
                      <Td align="right">
                        <button
                          type="button"
                          onClick={() =>
                            setQuotaForm({
                              num: "500",
                              unit: "GiB",
                              unlimited: !b.max,
                            })
                          }
                          style={{
                            background: "none",
                            border: 0,
                            color: "var(--acc)",
                            fontSize: 12.5,
                            cursor: "pointer",
                          }}
                        >
                          Sửa quota
                        </button>
                      </Td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </TableWrap>
        )}

        {tab === "keys" && (
          <TableWrap>
            <table
              data-tmin=""
              style={{ width: "100%", borderCollapse: "collapse" }}
            >
              <thead>
                <tr>
                  <Th width={230}>ACCESS KEY ID</Th>
                  <Th>QUYỀN</Th>
                  <Th width={168}>TRẠNG THÁI</Th>
                  <Th width={190} />
                </tr>
              </thead>
              <tbody>
                {KEYS.slice(0, 3).map((k) => (
                  <tr
                    key={k.id}
                    className="trHover"
                    style={{ borderBottom: "1px solid var(--line)" }}
                  >
                    <Td
                      style={{
                        ...monoStyle,
                        fontSize: 13,
                        color: "var(--dim)",
                      }}
                    >
                      {shortId(k.id)}
                    </Td>
                    <Td>
                      <div
                        style={{ display: "flex", gap: 5, flexWrap: "wrap" }}
                      >
                        {k.perms.slice(0, 3).map((p) => (
                          <Chip key={p}>{p}</Chip>
                        ))}
                      </div>
                    </Td>
                    <Td>
                      <PillDot view={pill(k.status)} />
                    </Td>
                    <Td align="right">
                      {/* TODO(slice#7): PATCH/DELETE /api/admin/keys/:pid — emergency actions */}
                      <button
                        type="button"
                        onClick={() => toast("Đã tạm khoá key")}
                        style={{
                          background: "none",
                          border: 0,
                          color: "var(--dim)",
                          fontSize: 12.5,
                          cursor: "pointer",
                          marginRight: 12,
                        }}
                      >
                        Tạm khoá
                      </button>
                      <button
                        type="button"
                        onClick={() => toast("Đã thu hồi key", "danger")}
                        style={{
                          background: "none",
                          border: 0,
                          color: "var(--dgr)",
                          fontSize: 12.5,
                          cursor: "pointer",
                        }}
                      >
                        Thu hồi
                      </button>
                    </Td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TableWrap>
        )}

        {tab === "activity" && (
          <div
            style={{
              background: "var(--panel)",
              border: "1px dashed var(--line2)",
              borderRadius: 12,
              padding: "60px 24px",
              textAlign: "center",
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 600 }}>
              Audit log — sắp có
            </div>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 6 }}>
              Nhật ký hoạt động nằm ở slice #6.
            </div>
          </div>
        )}
      </Page>

      {editName && (
        <FormModal width={460} onClose={() => setEditName(null)}>
          <FormHead title="Sửa thông tin user" sub={target.email} />
          <FormBody padding="18px 24px" gap={14}>
            <div>
              <div
                style={{ fontSize: 12, color: "var(--dim)", marginBottom: 6 }}
              >
                Tên
              </div>
              <input
                value={editName.name}
                onChange={(e) =>
                  setEditName({ ...editName, name: e.target.value })
                }
                placeholder="Tên hiển thị"
                style={{
                  width: "100%",
                  height: 38,
                  borderRadius: 8,
                  border: `1px solid ${
                    editName.touched && !editName.name.trim()
                      ? "var(--dgr)"
                      : "var(--line2)"
                  }`,
                  background: "var(--panel2)",
                  color: "var(--tx)",
                  padding: "0 12px",
                  fontSize: 13.5,
                }}
              />
              {editName.touched && !editName.name.trim() && (
                <div
                  style={{ fontSize: 12, color: "var(--dgr)", marginTop: 5 }}
                >
                  Tên không được để trống.
                </div>
              )}
            </div>
          </FormBody>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setEditName(null)} />
            <FormSubmit
              label="Lưu thay đổi"
              enabled
              onClick={() => {
                if (!editName.name.trim()) {
                  setEditName({ ...editName, touched: true });
                  return;
                }
                // TODO(slice#7): PATCH /api/admin/users/:pid {name}
                setTarget({ ...target, name: editName.name.trim() });
                setEditName(null);
                toast("Đã cập nhật thông tin user");
              }}
            />
          </FormFoot>
        </FormModal>
      )}

      {quotaForm && (
        <FormModal width={470} onClose={() => setQuotaForm(null)}>
          <FormHead
            title="Sửa quota"
            sub={`${target.email} đang dùng ${fmt(target.used)}.`}
          />
          <div style={{ padding: "18px 24px" }}>
            <QuotaFields
              num={quotaForm.num}
              unit={quotaForm.unit}
              unlimited={quotaForm.unlimited}
              onNum={(num) => setQuotaForm({ ...quotaForm, num })}
              onUnit={(unit) => setQuotaForm({ ...quotaForm, unit })}
              onUnlimited={(unlimited) =>
                setQuotaForm({ ...quotaForm, unlimited })
              }
            />
          </div>
          {quotaWarn && (
            <div
              style={{
                margin: "0 24px 8px",
                background: "rgba(214,192,67,.13)",
                border: "1px solid rgba(214,192,67,.4)",
                borderRadius: 9,
                padding: "12px 14px",
                fontSize: 12.5,
                color: "#E9DCA0",
                textWrap: "pretty",
              }}
            >
              User đang dùng {fmt(target.used)}, đặt quota {quotaForm.num}{" "}
              {quotaForm.unit} sẽ chặn mọi lần ghi mới cho tới khi họ xoá bớt.
            </div>
          )}
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setQuotaForm(null)} />
            <FormSubmit
              label="Lưu quota"
              enabled
              onClick={() => {
                // TODO(slice#7): PATCH /api/admin/users/:pid {max_bytes}
                setTarget({ ...target, max: quotaBytes });
                setQuotaForm(null);
                toast("Đã cập nhật quota");
              }}
            />
          </FormFoot>
        </FormModal>
      )}

      {nextRole && (
        <FormModal width={440} onClose={() => setNextRole(null)}>
          <FormHead title="Đổi role" sub={target.email} />
          <div style={{ padding: "18px 24px" }}>
            <select
              value={nextRole}
              onChange={(e) => setNextRole(e.target.value as "user" | "admin")}
              style={roleSelect}
            >
              <option value="user">User — chỉ dữ liệu của mình</option>
              <option value="admin">
                Admin — quản trị tài khoản &amp; quota
              </option>
            </select>
            {selfDemote && (
              <div
                style={{
                  marginTop: 12,
                  background: "var(--dgrSoft)",
                  border: "1px solid rgba(232,82,94,.4)",
                  borderRadius: 9,
                  padding: "12px 14px",
                  fontSize: 12.5,
                  color: "#FF9AA2",
                }}
              >
                Không thể tự hạ quyền chính mình.
              </div>
            )}
          </div>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setNextRole(null)} />
            <FormSubmit
              label="Đổi role"
              enabled={!selfDemote}
              onClick={() => {
                // TODO(slice#7): PATCH /api/admin/users/:pid {role}
                setTarget({ ...target, role: nextRole });
                setNextRole(null);
                toast("Đã đổi role");
              }}
            />
          </FormFoot>
        </FormModal>
      )}
    </>
  );
}
