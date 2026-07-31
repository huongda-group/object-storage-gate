import { Link, createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import {
  ErrorBanner,
  Mark,
  Sub,
  SubmitButton,
  TextField,
  Title,
  fieldStack,
  mono,
} from "../../components/auth-ui";
import { magicLink } from "../../lib/auth";

export const Route = createFileRoute("/_auth/magic-link")({
  component: MagicLink,
});

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;

function MagicLink() {
  const [email, setEmail] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);

  async function submit() {
    if (busy) return;
    if (!EMAIL_RE.test(email.trim())) {
      setErr("Email không hợp lệ.");
      return;
    }
    setBusy(true);
    setErr("");
    try {
      await magicLink(email.trim());
      setSent(true);
    } catch {
      setErr("Không gửi được liên kết. Thử lại sau.");
    }
    setBusy(false);
  }

  if (sent) {
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
          Đã gửi liên kết đăng nhập
        </div>
        <div
          style={{
            fontSize: 13.5,
            color: "var(--dim)",
            marginTop: 8,
            lineHeight: 1.6,
          }}
        >
          Mở hộp thư <span style={mono}>{email.trim()}</span> và bấm liên kết để
          vào console.
        </div>
      </div>
    );
  }

  return (
    <div>
      <Title>Đăng nhập bằng email</Title>
      <Sub>Không cần mật khẩu — hệ thống gửi một liên kết dùng một lần.</Sub>

      {err && <ErrorBanner>{err}</ErrorBanner>}

      <div style={fieldStack}>
        <TextField
          label="Email"
          type="email"
          placeholder="ten@congty.vn"
          value={email}
          onChange={(v) => {
            setEmail(v);
            setErr("");
          }}
          onEnter={submit}
        />
        <SubmitButton
          busy={busy}
          label="Gửi liên kết"
          busyLabel="Đang gửi…"
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
        <Link to="/login">Dùng mật khẩu</Link>
      </div>
    </div>
  );
}
