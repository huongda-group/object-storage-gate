// Ported from console-object-storage-gate/project/Access Keys.dc.html (mSecret).
// The secret is shown once: this component holds it, and closing drops it.
import { useState } from "react";
import { FormHead, FormModal } from "./Modal";
import { useToast } from "./Toast";
import { monoStyle } from "./ui";

const boxStyle = {
  flex: 1,
  height: 38,
  borderRadius: 8,
  border: "1px solid var(--line2)",
  background: "#0C0B0A",
  color: "var(--tx)",
  padding: "0 12px",
  fontSize: 13,
  display: "flex",
  alignItems: "center",
  ...monoStyle,
} as const;

const sideBtn = {
  height: 38,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--dim)",
  borderRadius: 8,
  fontSize: 12.5,
  cursor: "pointer",
} as const;

export function SecretRevealModal({
  keyId,
  secret,
  rotated,
  onClose,
}: {
  keyId: string;
  secret: string;
  rotated?: boolean;
  onClose: () => void;
}) {
  const toast = useToast();
  const [reveal, setReveal] = useState(false);
  const [ack, setAck] = useState(false);
  const [copied, setCopied] = useState<"id" | "secret" | null>(null);

  async function copy(text: string, which: "id" | "secret") {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // On an insecure origin, or with the permission denied, nothing was copied — and this
      // is the one secret the user cannot come back for.
      toast("Trình duyệt không cho copy. Chọn và copy thủ công.", "danger");
      return;
    }
    setCopied(which);
    toast("Đã copy vào clipboard");
    setTimeout(() => setCopied(null), 2600);
  }

  function downloadCsv() {
    const csv = `Access key ID,Secret access key\n${keyId},${secret}\n`;
    const url = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = "osgate-credentials.csv";
    // Firefox needs the anchor in the document, and revoking in the same tick races the
    // download in several browsers.
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 0);
    toast("Đã tải osgate-credentials.csv");
  }

  return (
    <FormModal width={560} onClose={() => ack && onClose()}>
      <FormHead
        title="Lưu secret key ngay bây giờ"
        sub={
          rotated ? (
            <>
              Key cũ đã chuyển sang{" "}
              <span style={{ color: "var(--dim)", fontWeight: 600 }}>
                Tạm khoá
              </span>
              . Sau khi đổi xong config, nhớ thu hồi key cũ.
            </>
          ) : undefined
        }
      />

      <div
        style={{
          margin: "16px 24px",
          background: "var(--warnSoft)",
          border: "1px solid rgba(214,192,67,.4)",
          borderRadius: 9,
          padding: "12px 14px",
          display: "flex",
          gap: 10,
        }}
      >
        <div
          style={{
            width: 16,
            height: 16,
            flex: "0 0 16px",
            borderRadius: "50%",
            background: "var(--warn)",
            color: "#1A0D03",
            fontSize: 11,
            fontWeight: 700,
            display: "grid",
            placeItems: "center",
            marginTop: 1,
          }}
        >
          !
        </div>
        <div style={{ fontSize: 12.5, color: "#E9DCA0", textWrap: "pretty" }}>
          Đây là lần duy nhất secret hiện ra. Đóng cửa sổ này là không lấy lại
          được.
        </div>
      </div>

      <div
        style={{
          padding: "0 24px",
          display: "flex",
          flexDirection: "column",
          gap: 14,
        }}
      >
        <div>
          <div
            style={{
              fontSize: 12,
              fontWeight: 500,
              color: "var(--dim)",
              marginBottom: 7,
            }}
          >
            Access Key ID
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <div style={boxStyle}>{keyId}</div>
            <button
              type="button"
              className="btnGhost"
              onClick={() => copy(keyId, "id")}
              style={{ ...sideBtn, width: 84 }}
            >
              {copied === "id" ? "✓ Copied" : "Copy"}
            </button>
          </div>
        </div>

        <div>
          <div
            style={{
              fontSize: 12,
              fontWeight: 500,
              color: "var(--dim)",
              marginBottom: 7,
            }}
          >
            Secret Access Key
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <div
              style={{
                ...boxStyle,
                letterSpacing: reveal ? "normal" : "1px",
              }}
            >
              {reveal ? secret : "•".repeat(40)}
            </div>
            <button
              type="button"
              className="btnGhost"
              onClick={() => setReveal(!reveal)}
              aria-label="Hiện secret"
              style={{
                ...sideBtn,
                width: 38,
                display: "grid",
                placeItems: "center",
              }}
            >
              <svg
                width="15"
                height="15"
                viewBox="0 0 16 16"
                aria-hidden="true"
              >
                <circle
                  cx="8"
                  cy="8"
                  r="2.4"
                  stroke="currentColor"
                  strokeWidth="1.4"
                  fill="none"
                />
                <ellipse
                  cx="8"
                  cy="8"
                  rx="6.6"
                  ry="4"
                  stroke="currentColor"
                  strokeWidth="1.4"
                  fill="none"
                />
              </svg>
            </button>
            <button
              type="button"
              className="btnGhost"
              onClick={() => copy(secret, "secret")}
              style={{ ...sideBtn, width: 84 }}
            >
              {copied === "secret" ? "✓ Copied" : "Copy"}
            </button>
          </div>
        </div>

        <div style={{ display: "flex", gap: 8 }}>
          <button
            type="button"
            onClick={downloadCsv}
            style={{
              height: 34,
              padding: "0 14px",
              border: "1px solid var(--line2)",
              background: "var(--panel2)",
              color: "var(--tx)",
              borderRadius: 8,
              fontSize: 12.5,
              cursor: "pointer",
            }}
          >
            Tải file .csv
          </button>
          <button
            type="button"
            onClick={() =>
              copy(
                `[osgate]\naws_access_key_id = ${keyId}\naws_secret_access_key = ${secret}`,
                "secret",
              )
            }
            style={{
              height: 34,
              padding: "0 14px",
              border: "1px solid var(--line2)",
              background: "var(--panel2)",
              color: "var(--tx)",
              borderRadius: 8,
              fontSize: 12.5,
              cursor: "pointer",
            }}
          >
            Snippet ~/.aws/credentials
          </button>
        </div>
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 16,
          padding: "18px 24px",
          borderTop: "1px solid var(--line)",
          marginTop: 20,
        }}
      >
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 9,
            fontSize: 13,
            color: "var(--tx)",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            checked={ack}
            onChange={(e) => setAck(e.target.checked)}
            style={{
              accentColor: "var(--acc)",
              width: 15,
              height: 15,
              cursor: "pointer",
            }}
          />
          Tôi đã lưu secret
        </label>
        <button
          type="button"
          onClick={() => ack && onClose()}
          disabled={!ack}
          style={{
            height: 36,
            padding: "0 18px",
            border: 0,
            borderRadius: 8,
            background: ack ? "var(--acc)" : "var(--panel2)",
            color: ack ? "var(--accTx)" : "var(--faint)",
            fontSize: 13,
            fontWeight: 600,
            cursor: ack ? "pointer" : "not-allowed",
          }}
        >
          Đóng
        </button>
      </div>
    </FormModal>
  );
}
