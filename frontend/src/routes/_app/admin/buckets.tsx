// Ported from console-object-storage-gate/project/Admin Buckets.dc.html.
// The prototype's pool form has no owner picker (the logic's OWNERS list is unused there), so this port follows the rendered form: name, quota, provider, public link.
import { Link, createFileRoute, redirect } from "@tanstack/react-router";
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
import { useToast } from "../../../components/Toast";
import { useShell } from "../../../components/shell";
import {
  H1,
  HeaderSearch,
  Page,
  PageAction,
  QuotaFields,
  RowMenu,
  RowMenuButton,
  TableWrap,
  Td,
  Th,
  menuItemStyle,
  monoStyle,
  useRowMenu,
} from "../../../components/ui";
import { validateBucketName } from "../../../lib/bucket-name";
import { colorFor, fmt, grp } from "../../../lib/format";
import {
  POOL_BUCKETS,
  PROVIDERS,
  type PoolBucket,
  UNITS,
} from "../../../lib/mock";

export const Route = createFileRoute("/_app/admin/buckets")({
  beforeLoad: ({ context }) => {
    if (context.user.role !== "admin") throw redirect({ to: "/" });
  },
  component: AdminBuckets,
});

type PoolForm = {
  editing: string | null;
  name: string;
  num: string;
  unit: keyof typeof UNITS;
  unlimited: boolean;
  provider: string;
  region: string;
  apiEndpoint: string;
  accessId: string;
  accessSecret: string;
  publicEnabled: boolean;
};

const EMPTY_FORM: PoolForm = {
  editing: null,
  name: "",
  num: "50",
  unit: "GiB",
  unlimited: false,
  provider: "internal",
  region: "",
  apiEndpoint: "",
  accessId: "",
  accessSecret: "",
  publicEnabled: false,
};

const fieldCol = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
  fontSize: 12,
  color: "var(--dim)",
} as const;

const fieldInput = {
  height: 38,
  borderRadius: 8,
  border: "1px solid var(--line2)",
  background: "var(--panel2)",
  color: "var(--tx)",
  padding: "0 12px",
  fontSize: 13.5,
  fontFamily: "'IBM Plex Mono',monospace",
} as const;

const statCard = {
  background: "var(--panel)",
  border: "1px solid var(--line)",
  borderRadius: 12,
  padding: "16px 18px",
} as const;

function AdminBuckets() {
  const { user, requestLogout } = useShell();
  const toast = useToast();
  const menu = useRowMenu();

  const [pools, setPools] = useState<PoolBucket[]>(POOL_BUCKETS);
  const [query, setQuery] = useState("");
  const [form, setForm] = useState<PoolForm | null>(null);
  const [secretVisible, setSecretVisible] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);

  const rows = query
    ? pools.filter((b) =>
        `${b.name} ${b.owner ?? "he thong"}`
          .toLowerCase()
          .includes(query.toLowerCase()),
      )
    : pools;

  const totalSize = pools.reduce((a, b) => a + b.used, 0);
  const totalObjects = pools.reduce((a, b) => a + b.objects, 0);
  const grantedQuota = pools
    .filter((b) => b.max)
    .reduce((a, b) => a + b.max, 0);
  const systemPools = pools.filter((b) => !b.owner).length;
  const owners = new Set(pools.filter((b) => b.owner).map((b) => b.owner)).size;

  const isEdit = !!form?.editing;
  const nameError = form
    ? validateBucketName(
        form.name,
        isEdit ? [] : pools.map((b) => b.name),
      ).replace("Bucket", "Pool")
    : "";
  const nameValid = !!form && form.name.length > 0 && !nameError;

  const publicLink = `https://public.osgate.vn/${form?.name || "(tên-bucket)"}`;

  function submit() {
    if (!form || !nameValid) return;
    const max = form.unlimited
      ? 0
      : Number.parseFloat(form.num || "0") * UNITS[form.unit];
    if (form.editing) {
      // TODO(slice#7): PATCH /api/admin/pools/:name
      setPools(
        pools.map((b) =>
          b.name === form.editing
            ? {
                ...b,
                max,
                provider: form.provider,
                region: form.region,
                apiEndpoint: form.apiEndpoint,
                accessId: form.accessId,
                accessSecret: form.accessSecret,
                publicEnabled: form.publicEnabled,
              }
            : b,
        ),
      );
      toast(`Đã lưu thay đổi cho ${form.name}`);
    } else {
      // TODO(slice#7): POST /api/admin/pools
      setPools([
        ...pools,
        {
          name: form.name,
          owner: null,
          used: 0,
          max,
          objects: 0,
          created: "vừa xong",
          full: "vừa xong",
          provider: form.provider,
          region: form.region,
          apiEndpoint: form.apiEndpoint,
          accessId: form.accessId,
          accessSecret: form.accessSecret,
          publicEnabled: form.publicEnabled,
        },
      ]);
      toast(`Đã tạo pool ${form.name}`);
    }
    setForm(null);
  }

  return (
    <>
      <Header
        user={user}
        onLogout={requestLogout}
        left={
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Link to="/admin" style={{ fontSize: 13, color: "var(--dim)" }}>
              Admin
            </Link>
            <span style={{ color: "var(--faint)", fontSize: 12 }}>/</span>
            <span style={{ fontSize: 13, fontWeight: 500, color: "var(--tx)" }}>
              Pool
            </span>
          </div>
        }
        right={
          <HeaderSearch
            value={query}
            onChange={setQuery}
            placeholder="Tìm pool hoặc chủ sở hữu…"
          />
        }
      />
      <Page maxWidth={1440}>
        <div
          style={{
            display: "flex",
            alignItems: "flex-end",
            justifyContent: "space-between",
            marginBottom: 18,
          }}
        >
          <div>
            <H1>Pool</H1>
            <div style={{ fontSize: 13, color: "var(--dim)", marginTop: 5 }}>
              Toàn bộ pool trên gateway · nơi tạo pool tổng không gắn với quota
              user
            </div>
          </div>
          <PageAction
            label="Tạo pool"
            onClick={() => {
              setSecretVisible(false);
              setForm({ ...EMPTY_FORM });
            }}
          />
        </div>

        <div
          data-grid="pstats"
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3,1fr)",
            gap: 14,
            marginBottom: 18,
          }}
        >
          <div style={statCard}>
            <div style={statLabel}>TỔNG POOL</div>
            <div style={statValue}>{grp(pools.length)}</div>
            <div style={statSub}>
              {owners} chủ sở hữu ·{" "}
              {systemPools > 0
                ? `${systemPools} pool hệ thống`
                : "không có pool hệ thống"}
            </div>
          </div>
          <div style={statCard}>
            <div style={statLabel}>TỔNG DUNG LƯỢNG</div>
            <div style={statValue}>{fmt(totalSize)}</div>
            <div style={statSub}>{fmt(grantedQuota)} quota đã cấp</div>
          </div>
          <div style={statCard}>
            <div style={statLabel}>TỔNG FILE</div>
            <div style={statValue}>{grp(totalObjects)}</div>
            <div style={statSub}>metadata rows trên mọi pool</div>
          </div>
        </div>

        <TableWrap>
          <table
            data-tmin=""
            style={{ width: "100%", borderCollapse: "collapse" }}
          >
            <thead>
              <tr>
                <Th>TÊN</Th>
                <Th>DUNG LƯỢNG</Th>
                <Th align="right">FILE</Th>
                <Th>TẠO LÚC</Th>
                <Th width={56} />
              </tr>
            </thead>
            <tbody>
              {rows.map((b) => {
                const pct = b.max ? Math.min(100, (b.used / b.max) * 100) : 0;
                const id = `p-${b.name}`;
                return (
                  <tr
                    key={b.name}
                    className="trHover"
                    style={{ borderBottom: "1px solid var(--line)" }}
                  >
                    <Td>
                      <Link
                        to="/buckets/$name"
                        params={{ name: b.name }}
                        style={{
                          ...monoStyle,
                          fontSize: "var(--fs)",
                          color: "var(--acc)",
                          fontWeight: 500,
                        }}
                      >
                        {b.name}
                      </Link>
                      <div
                        style={{
                          fontSize: 11.5,
                          color: "var(--faint)",
                          marginTop: 3,
                        }}
                      >
                        {b.owner ?? "hệ thống"}
                      </div>
                    </Td>
                    <Td>
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 12,
                        }}
                      >
                        {b.max ? (
                          <div
                            style={{
                              flex: 1,
                              height: 5,
                              borderRadius: 3,
                              background: "var(--line)",
                              overflow: "hidden",
                              display: "flex",
                            }}
                          >
                            <div
                              style={{
                                width: `${pct.toFixed(1)}%`,
                                background: colorFor(pct),
                              }}
                            />
                          </div>
                        ) : (
                          <div
                            style={{
                              flex: 1,
                              fontSize: 11,
                              color: "var(--acc)",
                              ...monoStyle,
                            }}
                          >
                            ∞ Không giới hạn
                          </div>
                        )}
                        <div
                          style={{
                            width: 150,
                            fontSize: 12.5,
                            color: "var(--dim)",
                            ...monoStyle,
                            textAlign: "right",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {b.max
                            ? `${fmt(b.used)} / ${fmt(b.max)}`
                            : `${fmt(b.used)} · ∞`}
                        </div>
                      </div>
                    </Td>
                    <Td
                      align="right"
                      style={{
                        ...monoStyle,
                        fontSize: 13,
                        color: "var(--dim)",
                      }}
                    >
                      {grp(b.objects)}
                    </Td>
                    <Td
                      title={b.full}
                      style={{ fontSize: 13, color: "var(--dim)" }}
                    >
                      {b.created}
                    </Td>
                    <Td
                      align="center"
                      style={{ padding: "0 8px", position: "relative" }}
                    >
                      <RowMenuButton onClick={(e) => menu.toggle(id, e, 126)} />
                      {menu.open === id && (
                        <RowMenu pos={menu.pos}>
                          <Link
                            to="/buckets/$name"
                            params={{ name: b.name }}
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={menu.close}
                          >
                            Mở object browser
                          </Link>
                          <button
                            type="button"
                            className="menuItem"
                            style={menuItemStyle}
                            onClick={() => {
                              menu.close();
                              setSecretVisible(false);
                              setForm({
                                editing: b.name,
                                name: b.name,
                                num: b.max
                                  ? String(Math.round(b.max / UNITS.GiB))
                                  : "50",
                                unit: "GiB",
                                unlimited: !b.max,
                                provider: b.provider ?? "internal",
                                region: b.region ?? "",
                                apiEndpoint: b.apiEndpoint ?? "",
                                accessId: b.accessId ?? "",
                                accessSecret: b.accessSecret ?? "",
                                publicEnabled: !!b.publicEnabled,
                              });
                            }}
                          >
                            Sửa pool
                          </button>
                          <button
                            type="button"
                            className="menuItemDanger"
                            style={{ ...menuItemStyle, color: "var(--dgr)" }}
                            onClick={() => {
                              menu.close();
                              setDeleting(b.name);
                            }}
                          >
                            Xoá pool
                          </button>
                        </RowMenu>
                      )}
                    </Td>
                  </tr>
                );
              })}
              {rows.length === 0 && (
                <tr>
                  <td
                    colSpan={5}
                    style={{
                      padding: "40px 16px",
                      textAlign: "center",
                      fontSize: 13,
                      color: "var(--dim)",
                    }}
                  >
                    Không tìm thấy pool phù hợp.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </TableWrap>
      </Page>

      {form && (
        <FormModal width={560} onClose={() => setForm(null)}>
          <FormHead
            title={isEdit ? "Sửa pool" : "Tạo pool"}
            sub={
              isEdit
                ? "Cập nhật chủ sở hữu và quota cho pool này."
                : 'Tên pool phải hợp lệ theo luật S3 và chưa tồn tại trên gateway. Chọn "Hệ thống" để tạo pool tổng không tính vào quota của user nào.'
            }
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
              Tên pool
              <input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                disabled={isEdit}
                placeholder="system-archive"
                style={{
                  ...fieldInput,
                  fontSize: 14,
                  border: `1px solid ${
                    nameError
                      ? "var(--dgr)"
                      : nameValid && !isEdit
                        ? "var(--ok)"
                        : "var(--line2)"
                  }`,
                  background: isEdit ? "var(--hover)" : "var(--panel2)",
                }}
              />
            </label>
            {nameError && (
              <div style={{ marginTop: -8, fontSize: 12.5, color: "#FF9AA2" }}>
                {nameError}
              </div>
            )}
            {nameValid && !isEdit && (
              <div
                style={{ marginTop: -8, fontSize: 12.5, color: "var(--ok)" }}
              >
                Tên hợp lệ
              </div>
            )}

            <div>
              <div
                style={{
                  fontSize: 12,
                  fontWeight: 500,
                  color: "var(--dim)",
                  marginBottom: 7,
                }}
              >
                Quota pool
              </div>
              <QuotaFields
                num={form.num}
                unit={form.unit}
                unlimited={form.unlimited}
                onNum={(num) => setForm({ ...form, num })}
                onUnit={(unit) => setForm({ ...form, unit })}
                onUnlimited={(unlimited) => setForm({ ...form, unlimited })}
              />
            </div>

            <div style={{ height: 1, background: "var(--line)" }} />

            <div>
              <div
                style={{
                  fontSize: 12,
                  fontWeight: 500,
                  color: "var(--dim)",
                  marginBottom: 7,
                }}
              >
                Provider lưu trữ
              </div>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1fr",
                  gap: 10,
                }}
              >
                <label style={fieldCol}>
                  Provider
                  <select
                    value={form.provider}
                    onChange={(e) =>
                      setForm({ ...form, provider: e.target.value })
                    }
                    style={{
                      ...fieldInput,
                      fontFamily: "inherit",
                      fontSize: 13,
                      padding: "0 10px",
                    }}
                  >
                    {PROVIDERS.map((p) => (
                      <option key={p.value} value={p.value}>
                        {p.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label style={fieldCol}>
                  Region
                  <input
                    value={form.region}
                    onChange={(e) =>
                      setForm({ ...form, region: e.target.value })
                    }
                    placeholder="ap-southeast-1"
                    style={fieldInput}
                  />
                </label>
              </div>
              <label style={{ ...fieldCol, marginTop: 10 }}>
                Provider API endpoint
                <input
                  value={form.apiEndpoint}
                  onChange={(e) =>
                    setForm({ ...form, apiEndpoint: e.target.value })
                  }
                  placeholder="https://s3.ap-southeast-1.amazonaws.com"
                  style={fieldInput}
                />
              </label>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1fr",
                  gap: 10,
                  marginTop: 10,
                }}
              >
                <label style={fieldCol}>
                  ID (access key)
                  <input
                    value={form.accessId}
                    onChange={(e) =>
                      setForm({ ...form, accessId: e.target.value })
                    }
                    placeholder="AKIA…"
                    style={fieldInput}
                  />
                </label>
                <label style={fieldCol}>
                  Secret
                  <div style={{ position: "relative" }}>
                    <input
                      type={secretVisible ? "text" : "password"}
                      value={form.accessSecret}
                      onChange={(e) =>
                        setForm({ ...form, accessSecret: e.target.value })
                      }
                      placeholder="••••••••••••"
                      style={{
                        ...fieldInput,
                        width: "100%",
                        padding: "0 58px 0 12px",
                      }}
                    />
                    <button
                      type="button"
                      onClick={() => setSecretVisible(!secretVisible)}
                      style={{
                        position: "absolute",
                        right: 4,
                        top: 4,
                        height: 30,
                        padding: "0 10px",
                        border: 0,
                        borderRadius: 6,
                        background: "var(--hover)",
                        color: "var(--dim)",
                        fontSize: 11.5,
                        cursor: "pointer",
                      }}
                    >
                      {secretVisible ? "Ẩn" : "Hiện"}
                    </button>
                  </div>
                </label>
              </div>
            </div>

            <div style={{ height: 1, background: "var(--line)" }} />

            <div>
              <label
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  justifyContent: "space-between",
                  gap: 14,
                  cursor: "pointer",
                }}
              >
                <span
                  style={{
                    fontSize: 12,
                    fontWeight: 500,
                    color: "var(--dim)",
                  }}
                >
                  Public link
                  <div
                    style={{
                      fontSize: 11.5,
                      color: "var(--faint)",
                      fontWeight: 400,
                      marginTop: 3,
                    }}
                  >
                    Cho phép truy cập object qua URL công khai, không cần ký
                    request
                  </div>
                </span>
                <input
                  type="checkbox"
                  checked={form.publicEnabled}
                  onChange={(e) =>
                    setForm({ ...form, publicEnabled: e.target.checked })
                  }
                  style={{
                    accentColor: "var(--acc)",
                    width: 16,
                    height: 16,
                    cursor: "pointer",
                    flex: "0 0 auto",
                    marginTop: 2,
                  }}
                />
              </label>
              {form.publicEnabled && (
                <div style={{ marginTop: 10, display: "flex", gap: 8 }}>
                  <input
                    value={publicLink}
                    readOnly
                    style={{
                      ...fieldInput,
                      flex: 1,
                      height: 36,
                      background: "var(--hover)",
                      color: "var(--dim)",
                      fontSize: 12.5,
                    }}
                  />
                  <button
                    type="button"
                    onClick={async () => {
                      try {
                        await navigator.clipboard?.writeText(publicLink);
                      } catch {
                        // clipboard unavailable — still confirm to the user
                      }
                      toast("Đã sao chép public link");
                    }}
                    style={{
                      height: 36,
                      padding: "0 14px",
                      border: "1px solid var(--line2)",
                      background: "var(--panel2)",
                      color: "var(--tx)",
                      borderRadius: 8,
                      fontSize: 12.5,
                      cursor: "pointer",
                    }}
                  >
                    Sao chép
                  </button>
                </div>
              )}
            </div>
          </FormBody>
          <FormFoot>
            <FormCancel onClick={() => setForm(null)} />
            <FormSubmit
              label={isEdit ? "Lưu thay đổi" : "Tạo pool"}
              enabled={nameValid}
              onClick={submit}
            />
          </FormFoot>
        </FormModal>
      )}

      {deleting && (
        <ConfirmDangerModal
          title={`Xoá pool ${deleting}`}
          body="Hành động này xoá cascade toàn bộ metadata object trong pool. Không hoàn tác được."
          target={deleting}
          confirmLabel="Xoá pool"
          onClose={() => setDeleting(null)}
          onConfirm={() => {
            // TODO(slice#7): DELETE /api/admin/pools/:name
            setPools(pools.filter((b) => b.name !== deleting));
            setDeleting(null);
            toast(`Đã xoá pool ${deleting}`, "danger");
          }}
        />
      )}
    </>
  );
}

const statLabel = {
  fontSize: 11,
  letterSpacing: ".1em",
  color: "var(--faint)",
  fontWeight: 600,
} as const;

const statValue = {
  fontSize: 24,
  fontWeight: 600,
  letterSpacing: "-.02em",
  marginTop: 10,
  fontFamily: "'IBM Plex Mono',monospace",
} as const;

const statSub = {
  fontSize: 12,
  color: "var(--dim)",
  marginTop: 6,
} as const;
