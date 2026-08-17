// Ported from console-object-storage-gate/project/Key Detail.dc.html.
import { Link, createFileRoute, useNavigate } from "@tanstack/react-router";
import { useCallback, useEffect, useState } from "react";
import { Header } from "../../../components/Header";
import { ConfirmDangerModal } from "../../../components/Modal";
import { SecretRevealModal } from "../../../components/SecretRevealModal";
import { useToast } from "../../../components/Toast";
import { useShell } from "../../../components/shell";
import {
  Page,
  PillDot,
  TableEmpty,
  TableWrap,
  monoStyle,
} from "../../../components/ui";
import { run } from "../../../lib/api-client";
import { pill, shortId } from "../../../lib/format";
import {
  type ApiKey,
  type Permission,
  getKey,
  revokeKey,
  rotateKey,
  updateKey,
} from "../../../lib/keys";

export const Route = createFileRoute("/_app/keys/$pid")({
  component: KeyDetail,
});

const PERM_DEFS: { action: Permission; label: string; desc: string }[] = [
  { action: "read", label: "Đọc", desc: "GetObject, HeadObject" },
  { action: "write", label: "Ghi", desc: "PutObject (ghi đè object cùng key)" },
  { action: "delete", label: "Xoá", desc: "DeleteObject" },
  { action: "list", label: "Liệt kê", desc: "ListObjectsV2, HeadBucket" },
  {
    action: "multipart",
    label: "Upload nhiều phần",
    desc: "File lớn (>5 GiB)",
  },
  {
    action: "presigned",
    label: "Link ký sẵn",
    desc: "Tạo URL tạm cho bên thứ ba",
  },
];

const PRESETS: { name: string; perms: Permission[] | null }[] = [
  { name: "Read-only", perms: ["read", "list"] },
  { name: "Read/Write", perms: ["read", "write", "list", "multipart"] },
  {
    name: "Full",
    perms: ["read", "write", "delete", "list", "multipart", "presigned"],
  },
  { name: "Tuỳ chỉnh", perms: null },
];

/** Canonical, order-independent key for comparing permission sets. */
const permKey = (perms: Permission[]) =>
  PERM_DEFS.filter((d) => perms.includes(d.action))
    .map((d) => d.action)
    .join(",");

function KeyDetail() {
  const { pid } = Route.useParams();
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const navigate = useNavigate();

  const [raw, setRaw] = useState<ApiKey | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [perms, setPerms] = useState<Permission[]>([]);
  const [savedPerms, setSavedPerms] = useState("");
  const [prefixes, setPrefixes] = useState<string[]>([]);
  const [savedPrefixes, setSavedPrefixes] = useState("");
  const [secret, setSecret] = useState<{
    keyId: string;
    secret: string;
  } | null>(null);
  const [revoking, setRevoking] = useState(false);

  /** Both editor blocks reset their "unsaved changes" baseline from the server. */
  const adopt = useCallback((key: ApiKey) => {
    setRaw(key);
    setPerms(key.permissions);
    setSavedPerms(permKey(key.permissions));
    setPrefixes(key.prefixes);
    setSavedPrefixes(key.prefixes.join(","));
  }, []);

  useEffect(() => {
    getKey(pid)
      .then(adopt)
      .catch(() => setLoadFailed(true));
  }, [pid, adopt]);

  const status = raw?.status ?? "active";

  async function savePerms() {
    const updated = await run(() => updateKey(pid, { permissions: perms }), {
      onError: (m) => toast(m, "danger"),
    });
    if (!updated) return;
    adopt(updated);
    toast("Đã lưu quyền");
  }

  async function savePrefixes() {
    const updated = await run(() => updateKey(pid, { prefixes }), {
      onError: (m) => toast(m, "danger"),
    });
    if (!updated) return;
    adopt(updated);
    toast("Đã lưu prefix");
  }

  async function doRotate() {
    const fresh = await run(() => rotateKey(pid), {
      onError: (m) => toast(m, "danger"),
    });
    if (!fresh) return;
    setSecret({ keyId: fresh.access_key_id, secret: fresh.secret });
  }

  async function doDisable() {
    const updated = await run(() => updateKey(pid, { status: "disabled" }), {
      onError: (m) => toast(m, "danger"),
    });
    if (!updated) return;
    adopt(updated);
    toast("Đã tạm khoá key");
  }

  // A failed revoke used to leave the confirm modal open with no message at all, so the user
  // clicked again.
  async function doRevoke() {
    const ok = await run(() => revokeKey(pid), {
      onError: (m) => toast(m, "danger"),
    });
    setRevoking(false);
    if (ok !== undefined) navigate({ to: "/keys" });
  }

  const header = (
    <Header
      user={user}
      onLogout={requestLogout}
      left={
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Link to="/keys" style={{ fontSize: 13, color: "var(--dim)" }}>
            Access Keys
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
            {shortId(pid)}
          </span>
        </div>
      }
    />
  );

  if (!raw) {
    if (!loadFailed) {
      return (
        <>
          {header}
          <Page>
            <div style={{ fontSize: 13, color: "var(--dim)" }}>Đang tải…</div>
          </Page>
        </>
      );
    }
    return (
      <>
        {header}
        <Page>
          <TableWrap>
            <TableEmpty
              title="Không tìm thấy access key"
              text={`Key "${pid}" không tồn tại trong tài khoản của bạn.`}
              action={
                <Link to="/keys" style={{ fontSize: 13, fontWeight: 500 }}>
                  Về danh sách key
                </Link>
              }
            />
          </TableWrap>
        </Page>
      </>
    );
  }

  const current = permKey(perms);
  const permsDirty = current !== savedPerms;
  const prefixDirty = prefixes.join(",") !== savedPrefixes;
  const writeWarn = perms.includes("write") && !perms.includes("read");
  const activePreset =
    PRESETS.find((p) => p.perms && p.perms.join(",") === current)?.name ??
    "Tuỳ chỉnh";

  return (
    <>
      {header}
      <Page>
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: 20,
            marginBottom: 20,
          }}
        >
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
              <h1
                style={{
                  fontSize: 20,
                  fontWeight: 600,
                  margin: 0,
                  ...monoStyle,
                  letterSpacing: "-.01em",
                }}
              >
                {raw.access_key_id}
              </h1>
              <PillDot view={pill(status)} />
            </div>
            <div
              style={{ fontSize: 12.5, color: "var(--faint)", marginTop: 8 }}
            >
              Secret chỉ hiện một lần lúc tạo. Mất thì xoay khoá — không lấy lại
              được ở đây.
            </div>
          </div>
          <div style={{ display: "flex", gap: 8, flex: "0 0 auto" }}>
            <button
              type="button"
              className="btnGhost"
              onClick={() => void doRotate()}
              style={headerBtn}
            >
              Xoay khoá
            </button>
            <button
              type="button"
              className="btnGhost"
              onClick={() => void doDisable()}
              style={headerBtn}
            >
              Tạm khoá
            </button>
          </div>
        </div>

        <div
          data-grid="two"
          style={{
            display: "grid",
            gridTemplateColumns: "1.25fr 1fr",
            gap: 14,
            alignItems: "start",
          }}
        >
          <div
            style={{
              background: "var(--panel)",
              border: "1px solid var(--line)",
              borderRadius: 12,
              overflow: "hidden",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "14px 18px",
                borderBottom: "1px solid var(--line)",
              }}
            >
              <div>
                <div style={{ fontSize: 13.5, fontWeight: 600 }}>Quyền</div>
                <div
                  style={{ fontSize: 12, color: "var(--faint)", marginTop: 3 }}
                >
                  6 action ánh xạ thẳng sang S3 API
                </div>
              </div>
              {permsDirty && (
                <span
                  style={{
                    fontSize: 11.5,
                    color: "var(--warn)",
                    fontWeight: 600,
                  }}
                >
                  Có thay đổi chưa lưu
                </span>
              )}
            </div>

            <div
              style={{
                display: "flex",
                gap: 6,
                padding: "12px 18px",
                borderBottom: "1px solid var(--line)",
                flexWrap: "wrap",
              }}
            >
              {PRESETS.map((p) => {
                const on = activePreset === p.name;
                return (
                  <button
                    key={p.name}
                    type="button"
                    onClick={() => p.perms && setPerms(p.perms)}
                    style={{
                      height: 28,
                      padding: "0 12px",
                      border: `1px solid ${on ? "var(--accLine)" : "var(--line2)"}`,
                      background: on ? "var(--accSoft)" : "transparent",
                      color: on ? "var(--acc)" : "var(--dim)",
                      borderRadius: 20,
                      fontSize: 12,
                      fontWeight: 500,
                      cursor: "pointer",
                    }}
                  >
                    {p.name}
                  </button>
                );
              })}
            </div>

            <div>
              {PERM_DEFS.map((d) => (
                <label
                  key={d.action}
                  className="rowHover"
                  style={{
                    display: "flex",
                    gap: 12,
                    alignItems: "flex-start",
                    padding: "12px 18px",
                    borderBottom: "1px solid var(--line)",
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={perms.includes(d.action)}
                    onChange={() =>
                      setPerms(
                        perms.includes(d.action)
                          ? perms.filter((p) => p !== d.action)
                          : [...perms, d.action],
                      )
                    }
                    style={{
                      accentColor: "var(--acc)",
                      width: 15,
                      height: 15,
                      marginTop: 2,
                      cursor: "pointer",
                    }}
                  />
                  <div>
                    <div style={{ fontSize: 13.5, fontWeight: 500 }}>
                      {d.label}{" "}
                      <span
                        style={{
                          ...monoStyle,
                          fontSize: 11.5,
                          color: "var(--faint)",
                          marginLeft: 4,
                        }}
                      >
                        {d.action}
                      </span>
                    </div>
                    <div
                      style={{
                        fontSize: 12.5,
                        color: "var(--dim)",
                        marginTop: 3,
                      }}
                    >
                      {d.desc}
                    </div>
                  </div>
                </label>
              ))}
            </div>

            {writeWarn && (
              <div
                style={{
                  padding: "12px 18px",
                  fontSize: 12.5,
                  color: "var(--dim)",
                  background: "var(--panel2)",
                }}
              >
                Key này ghi được nhưng không đọc lại được — đúng ý bạn chứ?
              </div>
            )}

            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                gap: 8,
                padding: "14px 18px",
                borderTop: "1px solid var(--line)",
              }}
            >
              <button
                type="button"
                onClick={() =>
                  setPerms(
                    savedPerms.split(",").filter(Boolean) as Permission[],
                  )
                }
                style={{
                  height: 32,
                  padding: "0 14px",
                  border: "1px solid var(--line2)",
                  background: "var(--panel2)",
                  color: "var(--dim)",
                  borderRadius: 8,
                  fontSize: 13,
                  cursor: "pointer",
                }}
              >
                Huỷ
              </button>
              <button
                type="button"
                disabled={!permsDirty}
                onClick={() => void savePerms()}
                style={saveBtn(permsDirty)}
              >
                Lưu quyền
              </button>
            </div>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <div
              style={{
                background: "var(--panel)",
                border: "1px solid var(--line)",
                borderRadius: 12,
                overflow: "hidden",
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  padding: "14px 18px",
                  borderBottom: "1px solid var(--line)",
                }}
              >
                <div>
                  <div style={{ fontSize: 13.5, fontWeight: 600 }}>Prefix</div>
                  <div
                    style={{
                      fontSize: 12,
                      color: "var(--faint)",
                      marginTop: 3,
                    }}
                  >
                    Giới hạn key chạm được những object nào
                  </div>
                </div>
                {prefixDirty && (
                  <span
                    style={{
                      fontSize: 11.5,
                      color: "var(--warn)",
                      fontWeight: 600,
                    }}
                  >
                    Chưa lưu
                  </span>
                )}
              </div>

              <div
                style={{
                  padding: "14px 18px",
                  display: "flex",
                  flexDirection: "column",
                  gap: 9,
                }}
              >
                {prefixes.length === 0 && (
                  <div
                    style={{
                      background: "var(--panel2)",
                      border: "1px dashed var(--line2)",
                      borderRadius: 9,
                      padding: 14,
                      fontSize: 12.5,
                      color: "var(--dim)",
                      textWrap: "pretty",
                    }}
                  >
                    Chưa giới hạn prefix — key này chạm được mọi object trong
                    tài khoản. Thêm prefix để thu hẹp.
                  </div>
                )}
                {prefixes.map((v, i) => {
                  // Spec §5.6: non-empty, no leading slash, ≤1024 chars.
                  const bad =
                    v.length > 0 && (v.startsWith("/") || v.length > 1024);
                  return (
                    <div
                      // biome-ignore lint/suspicious/noArrayIndexKey: prefix rows are positional and editable in place
                      key={i}
                      style={{ display: "flex", gap: 8, alignItems: "center" }}
                    >
                      <input
                        value={v}
                        onChange={(e) =>
                          setPrefixes(
                            prefixes.map((p, j) =>
                              j === i ? e.target.value : p,
                            ),
                          )
                        }
                        placeholder="images/*"
                        style={{
                          flex: 1,
                          height: 34,
                          borderRadius: 8,
                          border: `1px solid ${bad ? "var(--dgr)" : "var(--line2)"}`,
                          background: "var(--panel2)",
                          color: "var(--tx)",
                          padding: "0 11px",
                          fontSize: 13,
                          ...monoStyle,
                        }}
                      />
                      <button
                        type="button"
                        className="btnDanger"
                        onClick={() =>
                          setPrefixes(prefixes.filter((_, j) => j !== i))
                        }
                        aria-label="Xoá prefix"
                        style={{
                          width: 34,
                          height: 34,
                          border: "1px solid var(--line2)",
                          background: "var(--panel2)",
                          color: "var(--dim)",
                          borderRadius: 8,
                          cursor: "pointer",
                        }}
                      >
                        ×
                      </button>
                    </div>
                  );
                })}
                <button
                  type="button"
                  onClick={() => setPrefixes([...prefixes, ""])}
                  style={{
                    alignSelf: "flex-start",
                    height: 30,
                    padding: "0 12px",
                    border: "1px dashed var(--line2)",
                    background: "none",
                    color: "var(--dim)",
                    borderRadius: 8,
                    fontSize: 12.5,
                    cursor: "pointer",
                  }}
                >
                  + Thêm prefix
                </button>
                <div
                  style={{ fontSize: 12, color: "var(--faint)", marginTop: 2 }}
                >
                  Key được phép trên:{" "}
                  <span style={{ ...monoStyle, color: "var(--dim)" }}>
                    {prefixes.filter(Boolean).join(", ") || "toàn bộ tài khoản"}
                  </span>
                </div>
              </div>

              <div
                style={{
                  display: "flex",
                  justifyContent: "flex-end",
                  padding: "12px 18px",
                  borderTop: "1px solid var(--line)",
                }}
              >
                <button
                  type="button"
                  disabled={!prefixDirty}
                  onClick={() => void savePrefixes()}
                  style={saveBtn(prefixDirty)}
                >
                  Lưu prefix
                </button>
              </div>
            </div>

            <div
              style={{
                background: "var(--panel)",
                border: "1px solid rgba(232,82,94,.4)",
                borderRadius: 12,
                padding: 18,
              }}
            >
              <div
                style={{ fontSize: 13.5, fontWeight: 600, color: "#FF9AA2" }}
              >
                Danger zone
              </div>
              <div
                style={{
                  fontSize: 12.5,
                  color: "var(--dim)",
                  marginTop: 6,
                  textWrap: "pretty",
                }}
              >
                Thu hồi là vĩnh viễn — key không mở lại được. Nếu chỉ muốn dừng
                tạm, dùng "Tạm khoá".
              </div>
              <button
                type="button"
                onClick={() => setRevoking(true)}
                style={{
                  marginTop: 14,
                  height: 32,
                  padding: "0 14px",
                  border: "1px solid var(--dgr)",
                  background: "var(--dgrSoft)",
                  color: "#FF9AA2",
                  borderRadius: 8,
                  fontSize: 13,
                  fontWeight: 600,
                  cursor: "pointer",
                }}
              >
                Thu hồi key
              </button>
            </div>
          </div>
        </div>
      </Page>

      {secret && (
        <SecretRevealModal
          keyId={secret.keyId}
          secret={secret.secret}
          rotated
          onClose={() => {
            setSecret(null);
            toast("Access key đã sẵn sàng");
            // The old key is disabled now; the new one lives at its own pid.
            navigate({ to: "/keys" });
          }}
        />
      )}

      {revoking && (
        <ConfirmDangerModal
          title="Thu hồi access key"
          body="Thu hồi là vĩnh viễn — key không mở lại được. Mọi ứng dụng đang dùng key này sẽ nhận 403 ngay lập tức."
          target={raw.access_key_id}
          confirmLabel="Thu hồi key"
          onClose={() => setRevoking(false)}
          onConfirm={() => void doRevoke()}
        />
      )}
    </>
  );
}

const headerBtn = {
  height: 34,
  padding: "0 14px",
  border: "1px solid var(--line2)",
  background: "var(--panel)",
  color: "var(--tx)",
  borderRadius: 8,
  fontSize: 13,
  fontWeight: 500,
  cursor: "pointer",
} as const;

const saveBtn = (dirty: boolean) =>
  ({
    height: 32,
    padding: "0 14px",
    border: 0,
    borderRadius: 8,
    background: dirty ? "var(--acc)" : "var(--panel2)",
    color: dirty ? "var(--accTx)" : "var(--faint)",
    fontSize: 13,
    fontWeight: 600,
    cursor: dirty ? "pointer" : "not-allowed",
  }) as const;
