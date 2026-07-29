import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import {
  ErrorBanner,
  Mark,
  SecondaryButton,
  Sub,
  SubmitButton,
  TextField,
  Title,
  fieldStack,
  mono,
} from "../../components/auth-ui";
import { forgot } from "../../lib/auth";

export const Route = createFileRoute("/_auth/forgot")({ component: Forgot });

const EMAIL_RE = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;

function Forgot() {
  const navigate = useNavigate();
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
      // The server answers 200 whether or not the address exists — do not leak that.
      await forgot(email.trim());
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
          Đã gửi liên kết
        </div>
        <div
          style={{
            fontSize: 13.5,
            color: "var(--dim)",
            marginTop: 8,
            lineHeight: 1.6,
          }}
        >
          Kiểm tra hộp thư <span style={mono}>{email.trim()}</span>. Nếu sau 5
          phút chưa nhận được, kiểm tra thư mục spam hoặc thử lại.
        </div>
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 10,
            marginTop: 24,
          }}
        >
          <SubmitButton
            busy={false}
            label="Về trang đăng nhập"
            busyLabel=""
            onClick={() => navigate({ to: "/login" })}
          />
          <SecondaryButton label="Gửi lại" onClick={() => setSent(false)} />
        </div>
      </div>
    );
  }

  return (
    <div>
      <button
        type="button"
        className="btnGhost"
        onClick={() => navigate({ to: "/login" })}
        style={{
          background: "none",
          border: 0,
          padding: 0,
          cursor: "pointer",
          fontSize: 12.5,
          color: "var(--dim)",
          fontWeight: 500,
          marginBottom: 18,
        }}
      >
        ← Về trang đăng nhập
      </button>
      <Title>Đặt lại mật khẩu</Title>
      <Sub>
        Nhập email của bạn, hệ thống sẽ gửi liên kết đặt lại có hiệu lực 15
        phút.
      </Sub>

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
          label="Gửi liên kết đặt lại"
          busyLabel="Đang gửi…"
          onClick={submit}
        />
      </div>
    </div>
  );
}
