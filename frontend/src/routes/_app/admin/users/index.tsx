// Ported from console-object-storage-gate/project/Admin Users.dc.html.
// The mock users have no pid, so the detail route is keyed by email until slice #7 returns real pids from GET /api/admin/users.
import {
  Link,
  createFileRoute,
  redirect,
  useRouter,
} from "@tanstack/react-router";
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
  H1,
  HeaderSearch,
  Page,
  QuotaFields,
  RowMenu,
  RowMenuButton,
  TableWrap,
  Td,
  Th,
  menuItemStyle,
  monoStyle,
  useRowMenu,
} from "../../../../components/ui";
import {
  type AdminUser,
  createUser,
  listUsers,
  updateUser,
} from "../../../../lib/admin";
import { run } from "../../../../lib/api-client";
import { UNITS } from "../../../../lib/dashboard";
import { colorFor, fmt } from "../../../../lib/format";

export const Route = createFileRoute("/_app/admin/users/")({
  // UX guard only — AdminCaller is the real gate, on the server.
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  loader: () => listUsers(),
  component: AdminUsers,
});

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

type UserForm = {
  mode: "create" | "edit";
  pid: string | null;
  email: string;
  name: string;
  /// Only set when creating: the temporary password the user must replace at first login.
  password: string;
  role: "user" | "admin";
  num: string;
  unit: keyof typeof UNITS;
  unlimited: boolean;
  touched: boolean;
};

const NEW_USER: UserForm = {
  mode: "create",
  pid: null,
  email: "",
  name: "",
  password: "",
  role: "user",
  num: "100",
  unit: "GiB",
  unlimited: false,
  touched: false,
};

function AdminUsers() {
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const menu = useRowMenu();

  const router = useRouter();
  const users: AdminUser[] = Route.useLoaderData();
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [roleFilter, setRoleFilter] = useState<"all" | "user" | "admin">("all");
  const [nearFull, setNearFull] = useState(false);

  const [form, setForm] = useState<UserForm | null>(null);
  const [quotaFor, setQuotaFor] = useState<AdminUser | null>(null);
  const [quotaForm, setQuotaForm] = useState({
    num: "500",
    unit: "GiB" as keyof typeof UNITS,
    unlimited: false,
  });
  const [roleFor, setRoleFor] = useState<AdminUser | null>(null);
  const [nextRole, setNextRole] = useState<"user" | "admin">("user");

  let rows = users;
  if (roleFilter !== "all") rows = rows.filter((u) => u.role === roleFilter);
  if (query) {
    const q = query.toLowerCase();
    rows = rows.filter((u) => (u.email + u.name).toLowerCase().includes(q));
  }
  if (nearFull)
    rows = rows.filter(
      (u) => u.max_bytes > 0 && u.used_bytes / u.max_bytes >= 0.9,
    );

  const emailTrim = form?.email.trim() ?? "";
  const emailDup =
    form?.mode === "create" &&
    users.some((u) => u.email.toLowerCase() === emailTrim.toLowerCase());
  const emailErr = !emailTrim
    ? "Email không được để trống."
    : !EMAIL_RE.test(emailTrim)
      ? "Email không hợp lệ."
      : emailDup
        ? "Email đã tồn tại."
        : "";
  const nameErr = !form?.name.trim() || form.name.trim().length < 2;
  // The server enforces eight characters; say so before the round trip.
  const passwordErr =
    form?.mode === "create" && form.password.length < 8
      ? "Mật khẩu tạm phải từ 8 ký tự."
      : "";
  const formValid = !emailErr && !nameErr && !passwordErr;

  const quotaBytes = quotaForm.unlimited
    ? 0
    : Number.parseFloat(quotaForm.num || "0") * UNITS[quotaForm.unit];
  const quotaWarn =
    !!quotaFor && !quotaForm.unlimited && quotaBytes < quotaFor.used_bytes;

  // Spec §6.9: an admin may not demote themselves.
  const selfDemote =
    !!roleFor && roleFor.email === user.email && nextRole === "user";

  async function saveUser() {
    if (!form || busy) return;
    if (!formValid) {
      setForm({ ...form, touched: true });
      return;
    }
    setBusy(true);

    if (form.mode === "create") {
      const max = form.unlimited
        ? 0
        : Math.round(Number.parseFloat(form.num || "0") * UNITS[form.unit]);
      const created = await run(
        () =>
          createUser({
            email: emailTrim,
            name: form.name.trim(),
            password: form.password,
            role: form.role,
            max_bytes: max,
          }),
        { onError: (m) => toast(m, "danger") },
      );
      setBusy(false);
      if (!created) return;
      setForm(null);
      await router.invalidate();
      toast(
        `Đã tạo ${emailTrim}. Gửi mật khẩu tạm cho họ — hệ thống không gửi mail.`,
      );
      return;
    }

    const updated = await run(
      () =>
        updateUser(form.pid ?? "", {
          name: form.name.trim(),
          role: form.role,
        }),
      { onError: (m) => toast(m, "danger") },
    );
    setBusy(false);
    if (!updated) return;
    setForm(null);
    await router.invalidate();
    toast("Đã cập nhật thông tin user");
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Link to="/admin" style={{ fontSize: 13, color: "var(--dim)" }}>
              Admin
            </Link>
            <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
            <span style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
              Users
            </span>
          </div>
        }
        right={
          <HeaderSearch
            value={query}
            onChange={setQuery}
            placeholder="Tìm theo email hoặc tên…"
          />
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
            <H1>Users</H1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              {rows.length} user
            </div>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <select
              value={roleFilter}
              onChange={(e) =>
                setRoleFilter(e.target.value as "all" | "user" | "admin")
              }
              style={{
                height: 34,
                borderRadius: 8,
                border: "1px solid var(--line2)",
                background: "var(--panel)",
                color: "var(--dim)",
                padding: "0 10px",
                fontSize: 13,
                cursor: "pointer",
              }}
            >
              <option value="all">Mọi role</option>
              <option value="admin">Admin</option>
              <option value="user">User</option>
            </select>
            <button
              type="button"
              onClick={() => setNearFull(!nearFull)}
              style={{
                height: 34,
                padding: "0 14px",
                border: `1px solid ${nearFull ? "var(--accLine)" : "var(--line2)"}`,
                background: nearFull ? "var(--accSoft)" : "var(--panel)",
                color: nearFull ? "var(--acc)" : "var(--dim)",
                borderRadius: 8,
                fontSize: 13,
                fontWeight: 500,
                cursor: "pointer",
              }}
            >
              Quota ≥90%
            </button>
            <button
              type="button"
              onClick={() => setForm({ ...NEW_USER })}
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
              + Tạo user
            </button>
          </div>
        </div>

        <TableWrap>
          <table
            data-tmin=""
            style={{ width: "100%", borderCollapse: "collapse" }}
          >
            <thead>
              <tr>
                <Th>EMAIL</Th>
                <Th width={150}>TÊN</Th>
                <Th width={90}>ROLE</Th>
                <Th width={310}>DUNG LƯỢNG</Th>
                <Th align="center" width={140}>
                  MẬT KHẨU TẠM
                </Th>
                <Th width={56} />
              </tr>
            </thead>
            <tbody>
              {rows.map((u) => {
                const pct =
                  u.max_bytes > 0
                    ? Math.min(100, (u.used_bytes / u.max_bytes) * 100)
                    : 0;
                const id = `u-${u.email}`;
                return (
                  <tr
                    key={u.email}
                    className="trHover"
                    style={{ borderBottom: "1px solid var(--line)" }}
                  >
                    <Td>
                      <Link
                        to="/admin/users/$pid"
                        params={{ pid: u.pid }}
                        style={{ fontSize: "var(--fs)" }}
                      >
                        {u.email}
                      </Link>
                    </Td>
                    <Td style={{ fontSize: 13, color: "var(--dim)" }}>
                      {u.name}
                    </Td>
                    <Td>
                      <span
                        style={{
                          fontSize: 11,
                          fontWeight: 600,
                          padding: "3px 8px",
                          borderRadius: 5,
                          background:
                            u.role === "admin"
                              ? "var(--infoSoft)"
                              : "var(--panel2)",
                          color:
                            u.role === "admin" ? "var(--info)" : "var(--dim)",
                        }}
                      >
                        {u.role}
                      </span>
                    </Td>
                    <Td>
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 12,
                        }}
                      >
                        {u.max_bytes ? (
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
                                width: `${pct.toFixed(1)}%`,
                                background: colorFor(pct),
                              }}
                            />
                          </div>
                        ) : (
                          <div
                            style={{
                              flex: 1,
                              fontSize: 11,
                              color: "var(--acc)",
                              ...monoStyle,
                            }}
                          >
                            ∞ Không giới hạn
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
                          {u.max_bytes
                            ? `${fmt(u.used_bytes)} / ${fmt(u.max_bytes)} (${pct.toFixed(1)}%)`
                            : `${fmt(u.used_bytes)} đã dùng`}
                        </div>
                      </div>
                    </Td>
                    <Td
                      align="center"
                      style={{
                        fontSize: 12.5,
                        color: u.must_change_password
                          ? "var(--warn)"
                          : "var(--faint)",
                      }}
                    >
                      {u.must_change_password ? "chưa đổi" : "—"}
                    </Td>
                    <Td
                      align="center"
                      style={{ padding: "0 8px", position: "relative" }}
                    >
                      <RowMenuButton onClick={(e) => menu.toggle(id, e, 168)} />
                      {menu.open === id && (
                        <RowMenu pos={menu.pos}>
                          <Link
                            to="/admin/users/$pid"
                            params={{ pid: u.pid }}
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={menu.close}
                          >
                            Xem chi tiết
                          </Link>
                          <button
                            type="button"
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={() => {
                              menu.close();
                              setForm({
                                mode: "edit",
                                pid: u.pid,
                                email: u.email,
                                password: "",
                                name: u.name,
                                role: u.role,
                                num: "100",
                                unit: "GiB",
                                unlimited: false,
                                touched: false,
                              });
                            }}
                          >
                            Sửa thông tin
                          </button>
                          <button
                            type="button"
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={() => {
                              menu.close();
                              setQuotaFor(u);
                              setQuotaForm({
                                num: "500",
                                unit: "GiB",
                                unlimited: u.max_bytes === 0,
                              });
                            }}
                          >
                            Sửa quota
                          </button>
                          <button
                            type="button"
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={() => {
                              menu.close();
                              setRoleFor(u);
                              setNextRole(u.role);
                            }}
                          >
                            Đổi role
                          </button>
                        </RowMenu>
                      )}
                    </Td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </TableWrap>
      </Page>

      {form && (
        <FormModal onClose={() => setForm(null)}>
          <FormHead
            title={
              form.mode === "create" ? "Tạo user mới" : "Sửa thông tin user"
            }
            sub={
              form.mode === "create"
                ? "Tạo tài khoản user mới trong hệ thống."
                : form.email
            }
          />
          <FormBody padding="18px 24px" gap={14}>
            <div>
              <div
                style={{
                  fontSize: 12,
                  color: "var(--dim)",
                  marginBottom: 6,
                }}
              >
                Email
              </div>
              <input
                value={form.email}
                onChange={(e) => setForm({ ...form, email: e.target.value })}
                disabled={form.mode === "edit"}
                placeholder="ten@osgate.vn"
                style={{
                  width: "100%",
                  height: 38,
                  borderRadius: 8,
                  border: `1px solid ${
                    form.touched && emailErr ? "var(--dgr)" : "var(--line2)"
                  }`,
                  background: "var(--panel2)",
                  color: "var(--tx)",
                  padding: "0 12px",
                  fontSize: 13.5,
                }}
              />
              {form.touched && emailErr && (
                <div
                  style={{
                    fontSize: 12,
                    color: "var(--dgr)",
                    marginTop: 5,
                  }}
                >
                  {emailErr}
                </div>
              )}
            </div>

            {form.mode === "create" && (
              <div>
                <div
                  style={{ fontSize: 12, color: "var(--dim)", marginBottom: 6 }}
                >
                  Mật khẩu tạm
                </div>
                <input
                  value={form.password}
                  onChange={(e) =>
                    setForm({ ...form, password: e.target.value })
                  }
                  placeholder="Từ 8 ký tự"
                  autoComplete="new-password"
                  style={{
                    width: "100%",
                    height: 38,
                    borderRadius: 8,
                    border: `1px solid ${
                      form.touched && passwordErr
                        ? "var(--dgr)"
                        : "var(--line2)"
                    }`,
                    background: "var(--panel2)",
                    color: "var(--tx)",
                    padding: "0 12px",
                    fontSize: 13.5,
                    fontFamily: "'IBM Plex Mono',monospace",
                  }}
                />
                <div
                  style={{
                    fontSize: 12,
                    color:
                      form.touched && passwordErr
                        ? "var(--dgr)"
                        : "var(--faint)",
                    marginTop: 5,
                    lineHeight: 1.5,
                  }}
                >
                  {form.touched && passwordErr
                    ? passwordErr
                    : "Hệ thống không gửi mail — bạn tự chuyển mật khẩu này cho họ. Lần đăng nhập đầu, họ buộc phải đổi."}
                </div>
              </div>
            )}

            <div>
              <div
                style={{ fontSize: 12, color: "var(--dim)", marginBottom: 6 }}
              >
                Tên
              </div>
              <input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder="Tên hiển thị"
                style={{
                  width: "100%",
                  height: 38,
                  borderRadius: 8,
                  border: `1px solid ${
                    form.touched && nameErr ? "var(--dgr)" : "var(--line2)"
                  }`,
                  background: "var(--panel2)",
                  color: "var(--tx)",
                  padding: "0 12px",
                  fontSize: 13.5,
                }}
              />
              {form.touched && nameErr && (
                <div
                  style={{ fontSize: 12, color: "var(--dgr)", marginTop: 5 }}
                >
                  Tên không được để trống.
                </div>
              )}
            </div>

            <div>
              <div
                style={{ fontSize: 12, color: "var(--dim)", marginBottom: 6 }}
              >
                Role
              </div>
              <select
                value={form.role}
                onChange={(e) =>
                  setForm({ ...form, role: e.target.value as "user" | "admin" })
                }
                style={roleSelect}
              >
                <option value="user">User — chỉ dữ liệu của mình</option>
                <option value="admin">
                  Admin — quản trị tài khoản &amp; quota
                </option>
              </select>
            </div>

            {form.mode === "create" && (
              <div>
                <div
                  style={{ fontSize: 12, color: "var(--dim)", marginBottom: 6 }}
                >
                  Quota khởi tạo
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
            )}
          </FormBody>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setForm(null)} />
            <FormSubmit
              label={form.mode === "create" ? "Tạo user" : "Lưu thay đổi"}
              enabled
              onClick={() => void saveUser()}
            />
          </FormFoot>
        </FormModal>
      )}

      {quotaFor && (
        <FormModal width={470} onClose={() => setQuotaFor(null)}>
          <FormHead
            title="Sửa quota"
            sub={`${quotaFor.email} đang dùng ${fmt(quotaFor.used_bytes)}.`}
          />
          <div
            style={{
              padding: "18px 24px",
              display: "flex",
              gap: 10,
              alignItems: "center",
            }}
          >
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
              User đang dùng {fmt(quotaFor.used_bytes)}, đặt quota{" "}
              {quotaForm.num} {quotaForm.unit} sẽ chặn mọi lần ghi mới cho tới
              khi họ xoá bớt.
            </div>
          )}
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setQuotaFor(null)} />
            <FormSubmit
              label="Lưu quota"
              enabled
              onClick={() =>
                void (async () => {
                  const updated = await run(
                    () =>
                      updateUser(quotaFor.pid, {
                        max_bytes: Math.round(quotaBytes),
                      }),
                    { onError: (m) => toast(m, "danger") },
                  );
                  setQuotaFor(null);
                  if (!updated) return;
                  await router.invalidate();
                  toast("Đã cập nhật quota");
                })()
              }
            />
          </FormFoot>
        </FormModal>
      )}

      {roleFor && (
        <FormModal width={440} onClose={() => setRoleFor(null)}>
          <FormHead title="Đổi role" sub={roleFor.email} />
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
            <FormCancel onClick={() => setRoleFor(null)} />
            <FormSubmit
              label="Đổi role"
              enabled={!selfDemote}
              onClick={() =>
                void (async () => {
                  const updated = await run(
                    () => updateUser(roleFor.pid, { role: nextRole }),
                    { onError: (m) => toast(m, "danger") },
                  );
                  setRoleFor(null);
                  if (!updated) return;
                  await router.invalidate();
                  toast("Đã đổi role");
                })()
              }
            />
          </FormFoot>
        </FormModal>
      )}
    </>
  );
}

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
