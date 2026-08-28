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
import { ApiError, login, setToken } from "../../lib/auth";

export const Route = createFileRoute("/_auth/login")({ component: Login });

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;

function Login() {
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const [pw, setPw] = useState("");
  const [remember, setRemember] = useState(true);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit() {
    if (busy) return;
    if (!EMAIL_RE.test(email.trim())) {
      setErr("Email không hợp lệ.");
      return;
    }
    if (pw.length < 6) {
      setErr("Mật khẩu phải có ít nhất 6 ký tự.");
      return;
    }
    setBusy(true);
    setErr("");
    try {
      const res = await login(email.trim(), pw);
      setToken(res.token, remember);
      navigate({ to: "/" });
    } catch (e) {
      setBusy(false);
      setErr(
        e instanceof ApiError && e.status === 401
          ? "Email hoặc mật khẩu không đúng."
          : "Không đăng nhập được. Thử lại sau.",
      );
    }
  }

  return (
    <div>
      <Title>Đăng nhập</Title>
      <Sub>Dùng email công ty đã được cấp quyền trên gateway.</Sub>

      {err && <ErrorBanner>{err}</ErrorBanner>}

      <div style={fieldStack}>
        <TextField
          label="Email"
          type="email"
          autoComplete="username"
          placeholder="ten@congty.vn"
          value={email}
          onChange={(v) => {
            setEmail(v);
            setErr("");
          }}
          onEnter={submit}
        />

        <PasswordField
          label="Mật khẩu"
          value={pw}
          onChange={(v) => {
            setPw(v);
            setErr("");
          }}
          onEnter={submit}
        />

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <label
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              fontSize: 13,
              color: "var(--dim)",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
              style={{
                width: 15,
                height: 15,
                accentColor: "var(--acc)",
                cursor: "pointer",
              }}
            />
            Ghi nhớ thiết bị này
          </label>
        </div>

        <SubmitButton
          busy={busy}
          label="Đăng nhập"
          busyLabel="Đang xác thực…"
          onClick={submit}
        />
      </div>

      <div
        style={{
          textAlign: "center",
          marginTop: 16,
          fontSize: 13,
          color: "var(--dim)",
        }}
      >
        Tài khoản do quản trị viên cấp. Liên hệ quản trị viên nếu bạn quên mật
        khẩu.
      </div>
    </div>
  );
}
