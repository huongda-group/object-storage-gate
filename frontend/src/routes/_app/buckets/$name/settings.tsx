// Ported from console-object-storage-gate/project/Bucket Settings.dc.html.
import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Header } from "../../../../components/Header";
import { ConfirmDangerModal } from "../../../../components/Modal";
import { useToast } from "../../../../components/Toast";
import { useShell } from "../../../../components/shell";
import {
  Page,
  QuotaFields,
  TableEmpty,
  TableWrap,
  monoStyle,
} from "../../../../components/ui";
import { fmt } from "../../../../lib/format";
import { BUCKETS, UNITS } from "../../../../lib/mock";

export const Route = createFileRoute("/_app/buckets/$name/settings")({
  component: BucketSettings,
});

function BucketSettings() {
  const { name } = Route.useParams();
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const navigate = useNavigate();

  const bucket = BUCKETS.find((b) => b.name === name);

  const [num, setNum] = useState(() =>
    bucket?.max ? String(Math.round(bucket.max / UNITS.GiB)) : "50",
  );
  const [unit, setUnit] = useState<keyof typeof UNITS>("GiB");
  const [unlimited, setUnlimited] = useState(!bucket?.max);
  const [deleting, setDeleting] = useState(false);

  const header = (
    <Header
      user={user}
      onLogout={requestLogout}
      left={
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Link to="/buckets" style={{ fontSize: 13, color: "var(--dim)" }}>
            Buckets
          </Link>
          <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
          <Link
            to="/buckets/$name"
            params={{ name }}
            style={{ fontSize: 13, color: "var(--dim)", ...monoStyle }}
          >
            {name}
          </Link>
          <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            Cài đặt
          </span>
        </div>
      }
    />
  );

  if (!bucket) {
    return (
      <>
        {header}
        <Page>
          <TableWrap>
            <TableEmpty
              title="Không tìm thấy bucket"
              text={`Bucket "${name}" không tồn tại trong tài khoản của bạn.`}
              action={
                <Link to="/buckets" style={{ fontSize: 13, fontWeight: 500 }}>
                  Về danh sách bucket
                </Link>
              }
            />
          </TableWrap>
        </Page>
      </>
    );
  }

  const newMax = unlimited ? 0 : Number.parseFloat(num || "0") * UNITS[unit];
  // A quota under current usage is allowed by the API but blocks every new write.
  const belowUsed = !unlimited && newMax < bucket.used;

  return (
    <>
      {header}
      <Page>
        <h1
          style={{
            fontSize: 22,
            fontWeight: 600,
            letterSpacing: "-.02em",
            margin: "0 0 4px",
          }}
        >
          <span style={monoStyle}>{bucket.name}</span>
        </h1>
        <div style={{ fontSize: 13, color: "var(--dim)", marginBottom: 20 }}>
          Cài đặt bucket
        </div>

        <div
          style={{
            maxWidth: 640,
            display: "flex",
            flexDirection: "column",
            gap: 14,
          }}
        >
          <div
            style={{
              background: "var(--panel)",
              border: "1px solid var(--line)",
              borderRadius: 12,
              padding: 20,
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 600 }}>Quota bucket</div>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              Đang dùng {fmt(bucket.used)}. Đặt 0 hoặc tick "Không giới hạn" để
              bucket chỉ bị chặn bởi quota tài khoản.
            </div>
            <div style={{ marginTop: 16 }}>
              <QuotaFields
                num={num}
                unit={unit}
                unlimited={unlimited}
                onNum={setNum}
                onUnit={setUnit}
                onUnlimited={setUnlimited}
              />
            </div>
            {belowUsed && (
              <div
                style={{
                  marginTop: 12,
                  fontSize: 12.5,
                  color: "#FF9AA2",
                  textWrap: "pretty",
                }}
              >
                Bucket đang dùng {fmt(bucket.used)}; đặt quota {fmt(newMax)} sẽ
                chặn mọi lần ghi mới cho tới khi bạn xoá bớt object.
              </div>
            )}
            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                marginTop: 18,
              }}
            >
              <button
                type="button"
                onClick={() => {
                  // TODO(slice#7): PATCH /api/buckets/:pid {max_bytes}
                  toast("Đã lưu");
                }}
                style={{
                  height: 34,
                  padding: "0 16px",
                  border: 0,
                  borderRadius: 8,
                  background: "var(--acc)",
                  color: "var(--accTx)",
                  fontSize: 13,
                  fontWeight: 600,
                  cursor: "pointer",
                }}
              >
                Lưu quota
              </button>
            </div>
          </div>

          <div
            style={{
              background: "var(--panel)",
              border: "1px solid rgba(232,82,94,.4)",
              borderRadius: 12,
              padding: 20,
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 600, color: "#FF9AA2" }}>
              Danger zone
            </div>
            <div
              style={{
                fontSize: 13,
                color: "var(--dim)",
                marginTop: 6,
                maxWidth: "52ch",
                textWrap: "pretty",
              }}
            >
              Xoá bucket sẽ xoá cascade toàn bộ metadata object trong bucket
              này. Object trên object store gốc không được dọn tự động.
            </div>
            <button
              type="button"
              onClick={() => setDeleting(true)}
              style={{
                marginTop: 16,
                height: 34,
                padding: "0 16px",
                border: "1px solid var(--dgr)",
                background: "var(--dgrSoft)",
                color: "#FF9AA2",
                borderRadius: 8,
                fontSize: 13,
                fontWeight: 600,
                cursor: "pointer",
              }}
            >
              Xoá bucket
            </button>
          </div>
        </div>
      </Page>

      {deleting && (
        <ConfirmDangerModal
          title={`Xoá bucket ${bucket.name}`}
          body="Hành động này xoá cascade toàn bộ metadata object trong bucket. Không hoàn tác được."
          target={bucket.name}
          confirmLabel="Xoá bucket"
          onClose={() => setDeleting(false)}
          onConfirm={() => {
            // TODO(slice#7): DELETE /api/buckets/:pid
            setDeleting(false);
            navigate({ to: "/buckets" });
          }}
        />
      )}
    </>
  );
}
