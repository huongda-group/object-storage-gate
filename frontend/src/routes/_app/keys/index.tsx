// Ported from console-object-storage-gate/project/Access Keys.dc.html.
// The prototype opens the secret modal straight from "Tạo access key"; the create form here follows docs/ui/admin-ui-spec.md §6.5 (label, preset, prefix, expiry) using the same form-modal chrome as the other screens.
import { Link, createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { Header } from "../../../components/Header";
import {
  ConfirmDangerModal,
  FormBody,
  FormCancel,
  FormFoot,
  FormHead,
  FormModal,
  FormSubmit,
} from "../../../components/Modal";
import { SecretRevealModal } from "../../../components/SecretRevealModal";
import { useToast } from "../../../components/Toast";
import { useShell } from "../../../components/shell";
import {
  Chip,
  H1,
  Page,
  PageAction,
  PillDot,
  RowMenu,
  RowMenuButton,
  TableWrap,
  Td,
  Th,
  menuItemStyle,
  monoStyle,
  useRowMenu,
} from "../../../components/ui";
import { pill, shortId } from "../../../lib/format";
import {
  type ApiKey,
  type Permission,
  createKey as apiCreateKey,
  expiryLabel,
  listKeys,
  revokeKey,
  rotateKey,
  updateKey,
} from "../../../lib/keys";

export const Route = createFileRoute("/_app/keys/")({ component: AccessKeys });

const LABELS = ["primary", "backup", "temporary", "ci", "readonly"] as const;

export const PRESETS: { name: string; perms: Permission[] | null }[] = [
  { name: "Read-only", perms: ["read", "list"] },
  { name: "Read/Write", perms: ["read", "write", "list", "multipart"] },
  {
    name: "Full",
    perms: ["read", "write", "delete", "list", "multipart", "presigned"],
  },
  { name: "Tuỳ chỉnh", perms: null },
];

const EXPIRY_PRESETS = [
  { label: "Không hết hạn", days: 0 },
  { label: "7 ngày", days: 7 },
  { label: "30 ngày", days: 30 },
  { label: "90 ngày", days: 90 },
];

type NewKeyForm = {
  label: (typeof LABELS)[number];
  preset: string;
  prefix: string;
  expiryDays: number;
};

const EMPTY_FORM: NewKeyForm = {
  label: "primary",
  preset: "Read/Write",
  prefix: "",
  expiryDays: 0,
};

function AccessKeys() {
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const menu = useRowMenu();

  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [form, setForm] = useState<NewKeyForm | null>(null);
  const [secret, setSecret] = useState<{
    rotated: boolean;
    keyId: string;
    secret: string;
  } | null>(null);
  const [revoking, setRevoking] = useState<ApiKey | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    listKeys()
      .then(setKeys)
      .catch((e) => setLoadError(e instanceof Error ? e.message : String(e)));
  }, []);

  async function reload() {
    setKeys(await listKeys());
  }

  async function toggleStatus(k: ApiKey) {
    const next = k.status === "disabled" ? "active" : "disabled";
    await updateKey(k.pid, { status: next });
    await reload();
    toast(next === "active" ? "Đã mở lại key" : "Đã tạm khoá key");
  }

  async function doRotate(k: ApiKey) {
    const fresh = await rotateKey(k.pid);
    await reload();
    setSecret({
      rotated: true,
      keyId: fresh.access_key_id,
      secret: fresh.secret,
    });
  }

  async function doRevoke(pid: string) {
    await revokeKey(pid);
    await reload();
    setRevoking(null);
    toast("Đã thu hồi key", "danger");
  }

  async function copyId(id: string) {
    try {
      await navigator.clipboard?.writeText(id);
    } catch {
      // clipboard unavailable — still confirm to the user
    }
    setCopied(id);
    toast("Đã copy vào clipboard");
    setTimeout(() => setCopied(null), 2600);
  }

  async function createKey() {
    if (!form) return;
    const perms =
      PRESETS.find((p) => p.name === form.preset)?.perms ??
      (["read", "list"] as Permission[]);
    const fresh = await apiCreateKey({
      label: form.label,
      permissions: perms,
      prefixes: form.prefix.trim() ? [form.prefix.trim()] : [],
      expires_at: form.expiryDays
        ? new Date(Date.now() + form.expiryDays * 86_400_000).toISOString()
        : null,
    });
    await reload();
    setForm(null);
    setSecret({
      rotated: false,
      keyId: fresh.access_key_id,
      secret: fresh.secret,
    });
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
            Access Keys
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
            <H1>Access Keys</H1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              Secret chỉ hiện một lần lúc tạo. Mất thì xoay khoá.
            </div>
          </div>
          <PageAction
            label="Tạo access key"
            onClick={() => setForm({ ...EMPTY_FORM })}
          />
        </div>

        {loadError && (
          <div style={{ fontSize: 13, color: "var(--dgr)", marginBottom: 12 }}>
            Không tải được danh sách key: {loadError}
          </div>
        )}

        <TableWrap>
          <table
            data-tmin=""
            style={{ width: "100%", borderCollapse: "collapse" }}
          >
            <thead>
              <tr>
                <Th width={210}>ACCESS KEY ID</Th>
                <Th width={110}>NHÃN</Th>
                <Th>QUYỀN</Th>
                <Th width={170}>PHẠM VI</Th>
                <Th width={168}>TRẠNG THÁI</Th>
                <Th width={130}>HẾT HẠN</Th>
                <Th width={56} />
              </tr>
            </thead>
            <tbody>
              {keys.map((k) => {
                const id = `k-${k.pid}`;
                const scope = k.prefixes.length
                  ? k.prefixes[0] +
                    (k.prefixes.length > 1 ? `  +${k.prefixes.length - 1}` : "")
                  : "Toàn tài khoản";
                return (
                  <tr
                    key={k.pid}
                    className="trHover"
                    style={{ borderBottom: "1px solid var(--line)" }}
                  >
                    <Td>
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 8,
                        }}
                      >
                        <Link
                          to="/keys/$pid"
                          params={{ pid: k.pid }}
                          style={{
                            ...monoStyle,
                            fontSize: 13,
                            color: "var(--acc)",
                          }}
                        >
                          {shortId(k.access_key_id)}
                        </Link>
                        <button
                          type="button"
                          className="btnGhost"
                          onClick={() => copyId(k.access_key_id)}
                          aria-label="Copy access key id"
                          style={{
                            width: 22,
                            height: 22,
                            border: 0,
                            background: "none",
                            color: "var(--faint)",
                            borderRadius: 5,
                            cursor: "pointer",
                            fontSize: 12,
                          }}
                        >
                          {copied === k.access_key_id ? "✓" : "⧉"}
                        </button>
                      </div>
                    </Td>
                    <Td>
                      <Chip tone="dim">{k.label}</Chip>
                    </Td>
                    <Td>
                      <div
                        style={{ display: "flex", gap: 5, flexWrap: "wrap" }}
                      >
                        {k.permissions.slice(0, 3).map((p) => (
                          <Chip key={p}>{p}</Chip>
                        ))}
                        {k.permissions.length > 3 && (
                          <Chip tone="faint">+{k.permissions.length - 3}</Chip>
                        )}
                      </div>
                    </Td>
                    <Td
                      style={{
                        fontSize: 12.5,
                        color: "var(--dim)",
                        fontFamily: k.prefixes.length
                          ? "'IBM Plex Mono',monospace"
                          : "'IBM Plex Sans',sans-serif",
                      }}
                    >
                      {scope}
                    </Td>
                    <Td>
                      <PillDot view={pill(k.status)} />
                    </Td>
                    <Td
                      style={{
                        fontSize: 12.5,
                        color:
                          (k.days_until_expiry ?? 99) < 7 ||
                          k.status === "expired"
                            ? "var(--acc)"
                            : "var(--dim)",
                      }}
                    >
                      {expiryLabel(k.days_until_expiry) ?? "—"}
                    </Td>
                    <Td
                      align="center"
                      style={{ padding: "0 8px", position: "relative" }}
                    >
                      <RowMenuButton onClick={(e) => menu.toggle(id, e, 176)} />
                      {menu.open === id && (
                        <RowMenu pos={menu.pos}>
                          <Link
                            to="/keys/$pid"
                            params={{ pid: k.pid }}
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={menu.close}
                          >
                            Sửa quyền &amp; prefix
                          </Link>
                          <button
                            type="button"
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={() => {
                              menu.close();
                              void toggleStatus(k);
                            }}
                          >
                            {k.status === "disabled"
                              ? "Mở lại key"
                              : "Tạm khoá key"}
                          </button>
                          <button
                            type="button"
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={() => {
                              menu.close();
                              void doRotate(k);
                            }}
                          >
                            Xoay khoá
                          </button>
                          <div
                            style={{
                              height: 1,
                              background: "var(--line)",
                              margin: "4px 0",
                            }}
                          />
                          <button
                            type="button"
                            className="menuItemDanger"
                            style={{ ...menuItemStyle, color: "var(--dgr)" }}
                            onClick={() => {
                              menu.close();
                              setRevoking(k);
                            }}
                          >
                            Thu hồi vĩnh viễn
                          </button>
                        </RowMenu>
                      )}
                    </Td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </TableWrap>
      </Page>

      {form && (
        <FormModal onClose={() => setForm(null)}>
          <FormHead
            title="Tạo access key"
            sub="Secret chỉ hiện một lần sau khi tạo. Quyền và prefix sửa lại được bất cứ lúc nào."
          />
          <FormBody>
            <label
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 7,
                fontSize: 12,
                fontWeight: 500,
                color: "var(--dim)",
              }}
            >
              Nhãn
              <select
                value={form.label}
                onChange={(e) =>
                  setForm({
                    ...form,
                    label: e.target.value as (typeof LABELS)[number],
                  })
                }
                style={selectStyle}
              >
                {LABELS.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>
            </label>

            <div>
              <div
                style={{
                  fontSize: 12,
                  fontWeight: 500,
                  color: "var(--dim)",
                  marginBottom: 7,
                }}
              >
                Preset quyền
              </div>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {PRESETS.filter((p) => p.perms).map((p) => {
                  const on = form.preset === p.name;
                  return (
                    <button
                      key={p.name}
                      type="button"
                      onClick={() => setForm({ ...form, preset: p.name })}
                      style={{
                        height: 30,
                        padding: "0 12px",
                        borderRadius: 7,
                        border: `1px solid ${on ? "var(--accLine)" : "var(--line2)"}`,
                        background: on ? "var(--accSoft)" : "transparent",
                        color: on ? "var(--acc)" : "var(--dim)",
                        fontSize: 12.5,
                        fontWeight: 500,
                        cursor: "pointer",
                      }}
                    >
                      {p.name}
                    </button>
                  );
                })}
              </div>
              <div
                style={{
                  fontSize: 12,
                  color: "var(--faint)",
                  marginTop: 7,
                  ...monoStyle,
                }}
              >
                {PRESETS.find((p) => p.name === form.preset)?.perms?.join(", ")}
              </div>
            </div>

            <label
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 7,
                fontSize: 12,
                fontWeight: 500,
                color: "var(--dim)",
              }}
            >
              Prefix (tuỳ chọn — để trống là toàn tài khoản)
              <input
                value={form.prefix}
                onChange={(e) => setForm({ ...form, prefix: e.target.value })}
                placeholder="images/*"
                style={{
                  height: 38,
                  borderRadius: 8,
                  border: "1px solid var(--line2)",
                  background: "var(--panel2)",
                  color: "var(--tx)",
                  padding: "0 12px",
                  fontSize: 14,
                  ...monoStyle,
                }}
              />
            </label>

            <label
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 7,
                fontSize: 12,
                fontWeight: 500,
                color: "var(--dim)",
              }}
            >
              Hết hạn
              <select
                value={form.expiryDays}
                onChange={(e) =>
                  setForm({ ...form, expiryDays: Number(e.target.value) })
                }
                style={selectStyle}
              >
                {EXPIRY_PRESETS.map((p) => (
                  <option key={p.days} value={p.days}>
                    {p.label}
                  </option>
                ))}
              </select>
            </label>
          </FormBody>
          <FormFoot>
            <FormCancel onClick={() => setForm(null)} />
            <FormSubmit
              label="Tạo key"
              enabled
              onClick={() => void createKey()}
            />
          </FormFoot>
        </FormModal>
      )}

      {secret && (
        <SecretRevealModal
          keyId={secret.keyId}
          secret={secret.secret}
          rotated={secret.rotated}
          onClose={() => {
            setSecret(null);
            toast("Access key đã sẵn sàng");
          }}
        />
      )}

      {revoking && (
        <ConfirmDangerModal
          title="Thu hồi access key"
          body="Thu hồi là vĩnh viễn — key không mở lại được. Mọi ứng dụng đang dùng key này sẽ nhận 403 ngay lập tức."
          target={revoking.access_key_id}
          confirmLabel="Thu hồi key"
          onClose={() => setRevoking(null)}
          onConfirm={() => void doRevoke(revoking.pid)}
        />
      )}
    </>
  );
}

const selectStyle = {
  height: 38,
  borderRadius: 8,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--tx)",
  padding: "0 10px",
  fontSize: 13,
} as const;
