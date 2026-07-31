import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import {
  Mark,
  SecondaryButton,
  Sub,
  SubmitButton,
  Title,
} from "../../components/auth-ui";
import { verify } from "../../lib/auth";

export const Route = createFileRoute("/_auth/verify/$token")({
  component: Verify,
});

function Verify() {
  const { token } = Route.useParams();
  const navigate = useNavigate();
  const [state, setState] = useState<"pending" | "ok" | "bad">("pending");

  useEffect(() => {
    let alive = true;
    verify(token)
      .then(() => alive && setState("ok"))
      .catch(() => alive && setState("bad"));
    return () => {
      alive = false;
    };
  }, [token]);

  if (state === "pending") {
    return (
      <div>
        <Title>Đang xác thực…</Title>
        <Sub>Chờ một chút, đang kiểm tra liên kết.</Sub>
      </div>
    );
  }

  if (state === "ok") {
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
          Email đã xác thực
        </div>
        <Sub>Tài khoản đã sẵn sàng. Đăng nhập để vào console.</Sub>
        <div style={{ marginTop: 24, display: "grid" }}>
          <SubmitButton
            busy={false}
            label="Đăng nhập"
            busyLabel=""
            onClick={() => navigate({ to: "/login" })}
          />
        </div>
      </div>
    );
  }

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
        Liên kết không dùng được
      </div>
      <Sub>Token hỏng hoặc đã hết hạn. Đăng nhập rồi yêu cầu gửi lại.</Sub>
      <div style={{ marginTop: 24, display: "grid" }}>
        <SecondaryButton
          label="Về trang đăng nhập"
          onClick={() => navigate({ to: "/login" })}
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
        <Link to="/register">Đăng ký lại</Link>
      </div>
    </div>
  );
}
