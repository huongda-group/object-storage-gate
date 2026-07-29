// Ported from console-object-storage-gate/project/Access Keys.dc.html.
// The prototype opens the secret modal straight from "Tạo access key"; the create
// form here follows docs/ui/admin-ui-spec.md §6.5 (label, preset, prefix, expiry)
// using the same form-modal chrome as the other screens.
import { Link, createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
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
import { type KeyStatus, pill, shortId } from "../../../lib/format";
import {
  type AccessKey,
  KEYS,
  NEW_KEY,
  type Permission,
} from "../../../lib/mock";

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

  const [keys, setKeys] = useState<AccessKey[]>(KEYS);
  const [form, setForm] = useState<NewKeyForm | null>(null);
  const [secret, setSecret] = useState<{ rotated: boolean } | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  function setStatus(id: string, status: KeyStatus) {
    setKeys(keys.map((k) => (k.id === id ? { ...k, status } : k)));
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

  function createKey() {
    if (!form) return;
    const perms =
      PRESETS.find((p) => p.name === form.preset)?.perms ??
      (["read", "list"] as Permission[]);
    // TODO(slice#7): POST /api/keys {label, permissions, prefixes, expires_at}
    setKeys([
      ...keys,
      {
        id: NEW_KEY.id,
        label: form.label,
        status: "active",
        created: "vừa xong",
        exp: form.expiryDays ? `Còn ${form.expiryDays} ngày` : null,
        perms,
        prefixes: form.prefix.trim() ? [form.prefix.trim()] : [],
      },
    ]);
    setForm(null);
    setSecret({ rotated: false });
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
                const id = `k-${k.id}`;
                const scope = k.prefixes.length
                  ? k.prefixes[0] +
                    (k.prefixes.length > 1 ? `  +${k.prefixes.length - 1}` : "")
                  : "Toàn tài khoản";
                return (
                  <tr
                    key={k.id}
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
                          params={{ pid: k.id }}
                          style={{
                            ...monoStyle,
                            fontSize: 13,
                            color: "var(--acc)",
                          }}
                        >
                          {shortId(k.id)}
                        </Link>
                        <button
                          type="button"
                          className="btnGhost"
                          onClick={() => copyId(k.id)}
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
                          {copied === k.id ? "✓" : "⧉"}
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
                        {k.perms.slice(0, 3).map((p) => (
                          <Chip key={p}>{p}</Chip>
                        ))}
                        {k.perms.length > 3 && (
                          <Chip tone="faint">+{k.perms.length - 3}</Chip>
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
                          k.expSoon || k.status === "expired"
                            ? "var(--acc)"
                            : "var(--dim)",
                      }}
                    >
                      {k.exp ?? "—"}
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
                            params={{ pid: k.id }}
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
                              // TODO(slice#7): PATCH /api/keys/:pid {status}
                              const next =
                                k.status === "disabled" ? "active" : "disabled";
                              setStatus(k.id, next);
                              toast(
                                next === "active"
                                  ? "Đã mở lại key"
                                  : "Đã tạm khoá key",
                              );
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
                              // TODO(slice#7): POST /api/keys/:pid/rotate
                              setStatus(k.id, "disabled");
                              setSecret({ rotated: true });
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
                              setRevoking(k.id);
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
            <FormSubmit label="Tạo key" enabled onClick={createKey} />
          </FormFoot>
        </FormModal>
      )}

      {secret && (
        <SecretRevealModal
          keyId={NEW_KEY.id}
          secret={NEW_KEY.secret}
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
          target={revoking}
          confirmLabel="Thu hồi key"
          onClose={() => setRevoking(null)}
          onConfirm={() => {
            // TODO(slice#7): DELETE /api/keys/:pid
            setStatus(revoking, "revoked");
            setRevoking(null);
            toast("Đã thu hồi key", "danger");
          }}
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
