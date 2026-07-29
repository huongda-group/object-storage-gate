import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import {
  ErrorBanner,
  Mark,
  PasswordField,
  Sub,
  SubmitButton,
  Title,
  fieldStack,
} from "../../components/auth-ui";
import { reset } from "../../lib/auth";

export const Route = createFileRoute("/_auth/reset")({
  validateSearch: (search: Record<string, unknown>) => ({
    token: typeof search.token === "string" ? search.token : "",
  }),
  component: Reset,
});

function Reset() {
  const { token } = Route.useSearch();
  const navigate = useNavigate();
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit() {
    if (busy) return;
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
      await reset(token, pw);
      navigate({ to: "/login" });
    } catch {
      setBusy(false);
      setErr("Liên kết đã hết hạn hoặc không hợp lệ. Yêu cầu liên kết mới.");
    }
  }

  if (!token) {
    return (
      <div>
        <Mark kind="bad" />
        <div
          style={{
            fontSize: 22,
            fontWeight: 600,
            letterSpacing: "-.015em",
            marginTop: 16,
          }}
        >
          Liên kết không hợp lệ
        </div>
        <Sub>Thiếu token đặt lại. Yêu cầu một liên kết mới.</Sub>
        <div
          style={{
            textAlign: "center",
            marginTop: 24,
            fontSize: 13,
            color: "var(--dim)",
          }}
        >
          <Link to="/forgot">Gửi lại liên kết</Link>
        </div>
      </div>
    );
  }

  return (
    <div>
      <Title>Đặt mật khẩu mới</Title>
      <Sub>Mật khẩu mới sẽ thay thế mật khẩu cũ ngay lập tức.</Sub>

      {err && <ErrorBanner>{err}</ErrorBanner>}

      <div style={fieldStack}>
        <PasswordField
          label="Mật khẩu mới"
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
          label="Đặt lại mật khẩu"
          busyLabel="Đang lưu…"
          onClick={submit}
        />
      </div>
    </div>
  );
}
