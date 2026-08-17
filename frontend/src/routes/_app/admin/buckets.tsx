import { createFileRoute, redirect } from "@tanstack/react-router";
import { ComingSoon } from "../../../components/ComingSoon";
import { Header } from "../../../components/Header";
import { useShell } from "../../../components/shell";
import { Page } from "../../../components/ui";

export const Route = createFileRoute("/_app/admin/buckets")({
  // UX guard only — AdminCaller is the real gate, on the server.
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  component: AdminPools,
});

function AdminPools() {
  const { user, requestLogout } = useShell();

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            Pool
          </div>
        }
      />
      <Page>
        <ComingSoon
          title="Pool và backend store"
          reason="Cấu hình pool cần tầng proxy tới object store, hiện chưa được triển khai. Đừng nhập credential provider vào đây — bản trước thu thập access key và secret rồi vứt đi khi tải lại trang."
        />
      </Page>
    </>
  );
}
