import { Link, createFileRoute } from "@tanstack/react-router";
import { ComingSoon } from "../../../../components/ComingSoon";
import { Header } from "../../../../components/Header";
import { useShell } from "../../../../components/shell";
import { H1, Page, QuotaBar, monoStyle } from "../../../../components/ui";
import { getBucket } from "../../../../lib/buckets";
import { endpoint } from "../../../../lib/dashboard";
import { fmt, grp, quotaView } from "../../../../lib/format";

export const Route = createFileRoute("/_app/buckets/$pid/")({
  loader: ({ params }) => getBucket(params.pid),
  component: BucketDetail,
});

function BucketDetail() {
  const { pid } = Route.useParams();
  const bucket = Route.useLoaderData();
  const { user, requestLogout } = useShell();

  const q = quotaView(
    bucket.used_bytes,
    bucket.max_bytes,
    bucket.reserved_bytes,
  );
  const host = endpoint();

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Link to="/buckets" style={{ fontSize: 13, color: "var(--dim)" }}>
              Buckets
            </Link>
            <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
            <span
              style={{
                fontSize: 13,
                fontWeight: 500,
                color: "var(--tx)",
                ...monoStyle,
              }}
            >
              {bucket.name}
            </span>
          </div>
        }
      />
      <Page>
        <div
          style={{
            display: "flex",
            alignItems: "flex-end",
            justifyContent: "space-between",
            marginBottom: 18,
          }}
        >
          <div>
            <H1>{bucket.name}</H1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              {grp(bucket.object_count)} object · {fmt(bucket.used_bytes)} /{" "}
              {q.maxText}
              {bucket.public_enabled ? " · công khai" : ""}
            </div>
          </div>
          <Link
            to="/buckets/$pid/settings"
            params={{ pid }}
            style={{
              height: 34,
              padding: "0 14px",
              borderRadius: 8,
              border: "1px solid var(--line2)",
              background: "var(--panel)",
              color: "var(--dim)",
              fontSize: 13,
              display: "grid",
              placeItems: "center",
            }}
          >
            Cài đặt
          </Link>
        </div>

        <div style={{ maxWidth: 520 }}>
          <QuotaBar q={q} />
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              fontSize: 12,
              color: "var(--dim)",
              marginTop: 8,
            }}
          >
            <span>{q.pctText}</span>
            <span style={{ color: q.color, fontWeight: 600 }}>{q.state}</span>
          </div>
        </div>

        <div
          style={{
            marginTop: 24,
            padding: "14px 16px",
            border: "1px solid var(--line)",
            borderRadius: 10,
            background: "var(--panel)",
            maxWidth: 640,
          }}
        >
          <div style={{ fontSize: 12, color: "var(--faint)", fontWeight: 600 }}>
            KẾT NỐI
          </div>
          <pre
            style={{
              ...monoStyle,
              fontSize: 12.5,
              marginTop: 10,
              marginBottom: 0,
              whiteSpace: "pre-wrap",
              color: "var(--dim)",
            }}
          >
            {`aws s3 cp ./file.png s3://${bucket.name}/ \\\n  --endpoint-url ${host}`}
          </pre>
        </div>

        <ComingSoon
          title="Duyệt object"
          reason="Màn hình này cần API S3 của gateway, hiện chưa được triển khai. Dùng aws-cli hoặc rclone với access key của bạn khi tầng S3 lên."
        />
      </Page>
    </>
  );
}
