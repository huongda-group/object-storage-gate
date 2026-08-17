import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import {
  ErrorBanner,
  PasswordField,
  Sub,
  SubmitButton,
  TextField,
  Title,
  fieldStack,
} from "../../components/auth-ui";
import { setToken, setupAdmin } from "../../lib/auth";

export const Route = createFileRoute("/_auth/setup")({ component: Setup });

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;

function Setup() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit() {
    if (busy) return;
    if (!name.trim()) {
      setErr("Tên không được để trống.");
      return;
    }
    if (!EMAIL_RE.test(email.trim())) {
      setErr("Email không hợp lệ.");
      return;
    }
    if (pw.length < 8) {
      setErr("Mật khẩu phải có ít nhất 8 ký tự.");
      return;
    }
    if (pw !== pw2) {
      setErr("Hai mật khẩu không khớp.");
      return;
    }
    setBusy(true);
    setErr("");
    try {
      const res = await setupAdmin(name.trim(), email.trim(), pw);
      setToken(res.token);
      navigate({ to: "/" });
      return;
    } catch {
      setErr("Không tạo được tài khoản admin. Thử lại sau.");
    }
    setBusy(false);
  }

  return (
    <div>
      <Title>Tạo tài khoản admin</Title>
      <Sub>
        Hệ thống chưa có tài khoản nào. Tài khoản đầu tiên là admin, dùng để cấp
        quota và quản lý người dùng.
      </Sub>

      {err && <ErrorBanner>{err}</ErrorBanner>}

      <div style={fieldStack}>
        <TextField
          label="Tên"
          value={name}
          placeholder="An Nguyễn"
          onChange={(v) => {
            setName(v);
            setErr("");
          }}
          onEnter={submit}
        />
        <TextField
          label="Email"
          type="email"
          autoComplete="username"
          placeholder="admin@congty.vn"
          value={email}
          onChange={(v) => {
            setEmail(v);
            setErr("");
          }}
          onEnter={submit}
        />
        <PasswordField
          label="Mật khẩu"
          autoComplete="new-password"
          value={pw}
          onChange={(v) => {
            setPw(v);
            setErr("");
          }}
          onEnter={submit}
          right={
            <span style={{ fontSize: 12, color: "var(--faint)" }}>
              ≥8 ký tự
            </span>
          }
        />
        <PasswordField
          label="Xác nhận mật khẩu"
          autoComplete="new-password"
          value={pw2}
          onChange={(v) => {
            setPw2(v);
            setErr("");
          }}
          onEnter={submit}
        />
        <SubmitButton
          busy={busy}
          label="Tạo admin & vào console"
          busyLabel="Đang tạo…"
          onClick={submit}
        />
      </div>
    </div>
  );
}
