import { Link, createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import {
  ErrorBanner,
  Mark,
  PasswordField,
  SecondaryButton,
  Sub,
  SubmitButton,
  TextField,
  Title,
  fieldStack,
  mono,
} from "../../components/auth-ui";
import { register, resendVerification } from "../../lib/auth";

export const Route = createFileRoute("/_auth/register")({
  component: Register,
});

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;

function Register() {
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [resent, setResent] = useState(false);

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
      await register(name.trim(), email.trim(), pw);
      setDone(true);
    } catch {
      setErr("Không tạo được tài khoản. Thử lại sau.");
    }
    setBusy(false);
  }

  if (done) {
    return (
      <div>
        <Mark kind="ok" />
        <div
          style={{
            fontSize: 22,
            fontWeight: 600,
            letterSpacing: "-.015em",
            marginTop: 16,
          }}
        >
          Kiểm tra email của bạn
        </div>
        <div
          style={{
            fontSize: 13.5,
            color: "var(--dim)",
            marginTop: 8,
            lineHeight: 1.6,
          }}
        >
          Đã gửi liên kết xác thực tới <span style={mono}>{email.trim()}</span>.
          Xác thực xong mới đăng nhập được.
        </div>
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 10,
            marginTop: 24,
          }}
        >
          <SecondaryButton
            label={resent ? "Đã gửi lại" : "Gửi lại email xác thực"}
            onClick={async () => {
              await resendVerification(email.trim()).catch(() => {});
              setResent(true);
            }}
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
          <Link to="/login">Về trang đăng nhập</Link>
        </div>
      </div>
    );
  }

  return (
    <div>
      <Title>Tạo tài khoản</Title>
      <Sub>Sau khi xác thực email, admin sẽ cấp quota cho tài khoản.</Sub>

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
          label="Tạo tài khoản"
          busyLabel="Đang tạo…"
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
        Đã có tài khoản? <Link to="/login">Đăng nhập</Link>
      </div>
    </div>
  );
}
