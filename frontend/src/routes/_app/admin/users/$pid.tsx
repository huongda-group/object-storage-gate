import {
  Link,
  createFileRoute,
  redirect,
  useNavigate,
  useRouter,
} from "@tanstack/react-router";
import { useState } from "react";
import { ComingSoon } from "../../../../components/ComingSoon";
import { Header } from "../../../../components/Header";
import {
  ConfirmDangerModal,
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
  Page,
  Panel,
  PanelHead,
  QuotaFields,
  monoStyle,
} from "../../../../components/ui";
import {
  deleteUser,
  getUser,
  setUserPassword,
  updateUser,
} from "../../../../lib/admin";
import { run } from "../../../../lib/api-client";
import { UNITS } from "../../../../lib/dashboard";
import { fmt, quotaView } from "../../../../lib/format";

export const Route = createFileRoute("/_app/admin/users/$pid")({
  // UX guard only — AdminCaller is the real gate, on the server.
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  loader: ({ params }) => getUser(params.pid),
  component: AdminUserDetail,
});

const input = {
  width: "100%",
  height: 38,
  borderRadius: 8,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--tx)",
  padding: "0 12px",
  fontSize: 13.5,
} as const;

const rowStyle = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "13px 16px",
  borderTop: "1px solid var(--line)",
} as const;

const actionButton = {
  height: 30,
  padding: "0 12px",
  border: "1px solid var(--line2)",
  background: "var(--panel)",
  color: "var(--dim)",
  borderRadius: 8,
  fontSize: 12.5,
  cursor: "pointer",
} as const;

function AdminUserDetail() {
  const { pid } = Route.useParams();
  const target = Route.useLoaderData();
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const router = useRouter();
  const navigate = useNavigate();

  const [editName, setEditName] = useState<string | null>(null);
  const [quotaForm, setQuotaForm] = useState<{
    num: string;
    unit: keyof typeof UNITS;
    unlimited: boolean;
  } | null>(null);
  const [nextRole, setNextRole] = useState<"user" | "admin" | null>(null);
  const [newPassword, setNewPassword] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [busy, setBusy] = useState(false);

  const q = quotaView(
    target.used_bytes,
    target.max_bytes,
    target.reserved_bytes,
  );
  const isSelf = target.pid === user.pid;

  async function apply<T>(fn: () => Promise<T>, done: string) {
    if (busy) return;
    setBusy(true);
    const ok = await run(fn, { onError: (m) => toast(m, "danger") });
    setBusy(false);
    if (ok === undefined) return;
    await router.invalidate();
    toast(done);
    setEditName(null);
    setQuotaForm(null);
    setNextRole(null);
    setNewPassword(null);
  }

  async function destroy() {
    if (busy) return;
    setBusy(true);
    const ok = await run(() => deleteUser(pid), {
      onError: (m) => toast(m, "danger"),
    });
    setBusy(false);
    setDeleting(false);
    if (ok !== undefined) navigate({ to: "/admin/users" });
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Link
              to="/admin/users"
              style={{ fontSize: 13, color: "var(--dim)" }}
            >
              User
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
              {target.email}
            </span>
          </div>
        }
      />
      <Page>
        <H1>{target.name}</H1>
        <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
          <span style={monoStyle}>{target.email}</span> · {target.role} · tạo{" "}
          {new Date(target.created_at).toLocaleDateString("vi-VN")}
          {target.must_change_password ? " · đang dùng mật khẩu tạm" : ""}
        </div>

        <div style={{ maxWidth: 640, marginTop: 22 }}>
          <Panel>
            <PanelHead title="Tài khoản" />

            <div style={rowStyle}>
              <div>
                <div style={{ fontSize: 13.5 }}>Tên hiển thị</div>
                <div
                  style={{ fontSize: 12.5, color: "var(--dim)", marginTop: 3 }}
                >
                  {target.name}
                </div>
              </div>
              <button
                type="button"
                style={actionButton}
                onClick={() => setEditName(target.name)}
              >
                Sửa
              </button>
            </div>

            <div style={rowStyle}>
              <div>
                <div style={{ fontSize: 13.5 }}>Quota</div>
                <div
                  style={{ fontSize: 12.5, color: "var(--dim)", marginTop: 3 }}
                >
                  {fmt(target.used_bytes)} / {q.maxText} · {q.pctText}
                </div>
              </div>
              <button
                type="button"
                style={actionButton}
                onClick={() =>
                  setQuotaForm({
                    num: "100",
                    unit: "GiB",
                    unlimited: target.max_bytes === 0,
                  })
                }
              >
                Sửa quota
              </button>
            </div>

            <div style={rowStyle}>
              <div>
                <div style={{ fontSize: 13.5 }}>Role</div>
                <div
                  style={{ fontSize: 12.5, color: "var(--dim)", marginTop: 3 }}
                >
                  {target.role === "admin"
                    ? "Quản trị tài khoản và quota"
                    : "Chỉ dữ liệu của mình"}
                </div>
              </div>
              <button
                type="button"
                style={actionButton}
                disabled={isSelf}
                title={
                  isSelf ? "Không thể tự đổi role của chính mình" : undefined
                }
                onClick={() =>
                  setNextRole(target.role === "admin" ? "user" : "admin")
                }
              >
                Đổi role
              </button>
            </div>

            <div style={rowStyle}>
              <div>
                <div style={{ fontSize: 13.5 }}>Mật khẩu</div>
                <div
                  style={{
                    fontSize: 12.5,
                    color: "var(--dim)",
                    marginTop: 3,
                    maxWidth: "46ch",
                    lineHeight: 1.5,
                  }}
                >
                  Hệ thống không gửi mail. Đặt mật khẩu tạm rồi tự chuyển cho
                  họ; lần đăng nhập đầu họ buộc phải đổi.
                </div>
              </div>
              <button
                type="button"
                style={actionButton}
                onClick={() => setNewPassword("")}
              >
                Đặt lại
              </button>
            </div>

            <div style={rowStyle}>
              <div>
                <div style={{ fontSize: 13.5, color: "var(--dgr)" }}>
                  Xoá tài khoản
                </div>
                <div
                  style={{
                    fontSize: 12.5,
                    color: "var(--dim)",
                    marginTop: 3,
                    maxWidth: "46ch",
                    lineHeight: 1.5,
                  }}
                >
                  Xoá luôn bucket và metadata object của họ. Không hoàn tác
                  được.
                </div>
              </div>
              <button
                type="button"
                style={{ ...actionButton, color: "var(--dgr)" }}
                disabled={isSelf}
                title={isSelf ? "Không thể xoá chính mình" : undefined}
                onClick={() => setDeleting(true)}
              >
                Xoá
              </button>
            </div>
          </Panel>

          <ComingSoon
            title="Bucket và access key của user này"
            reason="Chưa có endpoint admin cho dữ liệu của tài khoản khác. Bản trước hiển thị danh sách giả kèm nút “tạm khoá key” và “thu hồi key” chỉ hiện toast — một admin tin rằng sự cố đã được khoanh vùng trong khi không có gì xảy ra."
          />
        </div>
      </Page>

      {editName !== null && (
        <FormModal width={440} onClose={() => setEditName(null)}>
          <FormHead title="Sửa tên" sub={target.email} />
          <div style={{ padding: "18px 24px" }}>
            <input
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              style={input}
            />
          </div>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setEditName(null)} />
            <FormSubmit
              label="Lưu"
              enabled={editName.trim().length >= 2 && !busy}
              onClick={() =>
                void apply(
                  () => updateUser(pid, { name: editName.trim() }),
                  "Đã cập nhật tên",
                )
              }
            />
          </FormFoot>
        </FormModal>
      )}

      {quotaForm && (
        <FormModal width={470} onClose={() => setQuotaForm(null)}>
          <FormHead
            title="Sửa quota"
            sub={`${target.email} đang dùng ${fmt(target.used_bytes)}.`}
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
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setQuotaForm(null)} />
            <FormSubmit
              label="Lưu quota"
              enabled={!busy}
              onClick={() =>
                void apply(
                  () =>
                    updateUser(pid, {
                      max_bytes: quotaForm.unlimited
                        ? 0
                        : Math.round(
                            Number.parseFloat(quotaForm.num || "0") *
                              UNITS[quotaForm.unit],
                          ),
                    }),
                  "Đã cập nhật quota",
                )
              }
            />
          </FormFoot>
        </FormModal>
      )}

      {nextRole && (
        <FormModal width={440} onClose={() => setNextRole(null)}>
          <FormHead title="Đổi role" sub={target.email} />
          <div style={{ padding: "18px 24px", fontSize: 13.5 }}>
            Đổi role thành <strong>{nextRole}</strong>.
            {nextRole === "admin"
              ? " Admin quản trị được mọi tài khoản và quota."
              : " Server từ chối nếu đây là admin cuối cùng."}
          </div>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setNextRole(null)} />
            <FormSubmit
              label="Đổi role"
              enabled={!busy}
              onClick={() =>
                void apply(
                  () => updateUser(pid, { role: nextRole }),
                  "Đã đổi role",
                )
              }
            />
          </FormFoot>
        </FormModal>
      )}

      {newPassword !== null && (
        <FormModal width={470} onClose={() => setNewPassword(null)}>
          <FormHead
            title="Đặt mật khẩu tạm"
            sub={`${target.email} sẽ buộc phải đổi ở lần đăng nhập kế tiếp.`}
          />
          <div style={{ padding: "18px 24px" }}>
            <input
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              placeholder="Từ 8 ký tự"
              autoComplete="new-password"
              style={{ ...input, fontFamily: "'IBM Plex Mono',monospace" }}
            />
            <div
              style={{
                fontSize: 12,
                color: "var(--faint)",
                marginTop: 6,
                lineHeight: 1.5,
              }}
            >
              Copy trước khi đóng — máy chủ chỉ lưu bản băm.
            </div>
          </div>
          <FormFoot padding="14px 24px">
            <FormCancel onClick={() => setNewPassword(null)} />
            <FormSubmit
              label="Đặt mật khẩu"
              enabled={newPassword.length >= 8 && !busy}
              onClick={() =>
                void apply(
                  () => setUserPassword(pid, newPassword),
                  "Đã đặt mật khẩu tạm",
                )
              }
            />
          </FormFoot>
        </FormModal>
      )}

      {deleting && (
        <ConfirmDangerModal
          title={`Xoá tài khoản ${target.email}`}
          body="Xoá luôn bucket và metadata object của họ. Không hoàn tác được."
          target={target.email}
          confirmLabel="Xoá tài khoản"
          onClose={() => setDeleting(false)}
          onConfirm={() => void destroy()}
        />
      )}
    </>
  );
}
