import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import {
  ErrorBanner,
  PasswordField,
  Sub,
  SubmitButton,
  Title,
  fieldStack,
} from "../../components/auth-ui";
import { ApiError, changePassword } from "../../lib/auth";

export const Route = createFileRoute("/_app/change-password")({
  component: ChangePassword,
});

function ChangePassword() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  async function submit() {
    if (busy) return;
    if (next.length < 8) {
      setErr("Mật khẩu mới phải từ 8 ký tự.");
      return;
    }
    if (next !== confirm) {
      setErr("Hai lần nhập không khớp.");
      return;
    }
    setBusy(true);
    setErr("");
    try {
      await changePassword(current, next);
      // A full reload is the simplest way to drop the cached user, which still says the
      // password must change.
      globalThis.location.assign("/");
    } catch (e) {
      setBusy(false);
      setErr(
        e instanceof ApiError && e.status === 401
          ? "Mật khẩu hiện tại không đúng."
          : "Không đổi được mật khẩu. Thử lại sau.",
      );
    }
  }

  return (
    <div style={{ maxWidth: 380, margin: "48px auto" }}>
      <Title>Đổi mật khẩu</Title>
      <Sub>
        Tài khoản đang dùng mật khẩu tạm do quản trị viên cấp. Đặt mật khẩu
        riêng để tiếp tục.
      </Sub>

      {err && <ErrorBanner>{err}</ErrorBanner>}

      <div style={fieldStack}>
        <PasswordField
          label="Mật khẩu hiện tại"
          autoComplete="current-password"
          value={current}
          onChange={(v) => {
            setCurrent(v);
            setErr("");
          }}
          onEnter={submit}
        />
        <PasswordField
          label="Mật khẩu mới"
          autoComplete="new-password"
          value={next}
          onChange={(v) => {
            setNext(v);
            setErr("");
          }}
          onEnter={submit}
        />
        <PasswordField
          label="Nhập lại mật khẩu mới"
          autoComplete="new-password"
          value={confirm}
          onChange={(v) => {
            setConfirm(v);
            setErr("");
          }}
          onEnter={submit}
        />
        <SubmitButton
          busy={busy}
          label="Đổi mật khẩu"
          busyLabel="Đang đổi…"
          onClick={submit}
        />
      </div>
    </div>
  );
}
