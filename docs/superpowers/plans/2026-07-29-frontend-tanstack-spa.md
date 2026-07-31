# Frontend → TanStack Router SPA Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the loco starter splash in `frontend/` with a TanStack Router SPA that implements all 13 console screens from `console-object-storage-gate/project/`.

**Architecture:** File-based TanStack Router on the existing rsbuild setup. Two route shells: `_auth` (centred card, unauthenticated) and `_app` (sidebar + sticky header, token-guarded). Each design prototype is a `.dc.html` file containing a template plus a `class Component extends DCLogic` with state and mock data; porting is mechanical — template becomes JSX with the inline styles copied verbatim, `renderVals()` becomes derived values, mock data moves to `src/lib/mock.ts`. Auth talks to the real loco endpoints; everything else reads mocks until slice #7 ships the admin API.

**Tech Stack:** React 18, TypeScript, rsbuild (rspack), `@tanstack/react-router` + `@tanstack/router-plugin/rspack`, vitest (lib tests only), biome.

**Spec:** `docs/superpowers/specs/2026-07-29-frontend-tanstack-spa-design.md`
**UI spec (behaviour, data shapes, copy):** `docs/ui/admin-ui-spec.md`
**Design prototypes (visual source of truth):** `console-object-storage-gate/project/*.dc.html`

## Global Constraints

- **Never commit.** Project rule in `CLAUDE.md`: leave changes staged or unstaged; the user commits. Every task's final step is `git add` only.
- **Pixel-exact port.** Copy every colour, size, radius, gap, font-size and weight from the prototype verbatim. Do not "improve" spacing, restyle, swap an icon, or re-word Vietnamese copy. If a value looks wrong, port it as-is and mention it in the task report.
- **Do not port `support.js`** — it is the design-tool runtime. Do not render the prototypes in a browser; read the HTML/CSS source.
- **No new dependencies** beyond `@tanstack/react-router`, `@tanstack/router-plugin`, `vitest`. No CSS framework, no component library, no icon package (icons are inline SVG in the prototypes).
- **Inline styles stay inline** (`style={{...}}`). Only `:hover` / `:focus` / `@media` move to `src/styles.css`, because inline styles cannot express them.
- **All prototype `<a href="X.dc.html">` become `<Link to="/route">`** from `@tanstack/react-router`. No raw `href` navigation inside the app.
- **Shared helpers live in `src/lib/format.ts` only.** Never re-declare `fmt`, `grp`, `colorFor`, `quotaView`, `pill`, or `shortId` inside a screen.
- **Drop the prototypes' `demo` state** (the `normal`/`loading`/`empty`/`forbidden` switcher). Keep the loading / empty / error branches themselves and drive them from data.
- Every task verifies with `cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/` — all three clean.
- Package manager is **pnpm**, run from `frontend/`.

## File Structure

| File | Responsibility |
|---|---|
| `frontend/package.json` | deps + scripts (`dev`, `build`, `lint`, `test`) |
| `frontend/rsbuild.config.ts` | rspack router plugin, `/api` proxy (keep), html title |
| `frontend/src/main.tsx` | router creation + mount (replaces `index.tsx`) |
| `frontend/src/styles.css` | `:root` tokens, IBM Plex import, hover/focus classes, `@media` rules |
| `frontend/src/lib/format.ts` | `fmt`, `grp`, `colorFor`, `quotaView`, `pill`, `shortId` |
| `frontend/src/lib/format.test.ts` | vitest unit tests for the above |
| `frontend/src/lib/auth.ts` | token storage, `api()` fetch wrapper, `login/register/forgot/reset/current` calls |
| `frontend/src/lib/auth.test.ts` | vitest tests for token storage + `api()` header |
| `frontend/src/lib/mock.ts` | typed fixtures: buckets, keys, objects, admin users, system stats |
| `frontend/src/components/Sidebar.tsx` | collapsible nav, `localStorage` key `osg_collapsed` |
| `frontend/src/components/Header.tsx` | page title slot, avatar menu, logout confirm |
| `frontend/src/components/Toast.tsx` | `useToast()` + fixed-position toast |
| `frontend/src/components/Modal.tsx` | overlay + centred panel shell |
| `frontend/src/components/QuotaBar.tsx` | used/reserved bar from a `quotaView` result |
| `frontend/src/components/Pill.tsx` | status pill from a `pill()` result |
| `frontend/src/components/Copyable.tsx` | copy-to-clipboard button + `✓`/`⧉` state |
| `frontend/src/routes/__root.tsx` | root route, `<Outlet/>`, devtools off |
| `frontend/src/routes/_auth.tsx` + `_auth/*.tsx` | auth shell + login/register/forgot/reset/verify/magic-link |
| `frontend/src/routes/_app.tsx` | guard + sidebar/header layout |
| `frontend/src/routes/_app/*.tsx` | one file per console screen (see route map in the spec) |
| `src/views/auth.rs` | add `role` + `max_bytes` to `CurrentResponse` (Task 4) |

Deleted: `frontend/src/index.tsx`, `frontend/src/LocoSplash.tsx`, `frontend/src/index.css`.

---

### Task 1: Scaffold router + tokens

**Files:**
- Modify: `frontend/package.json`
- Modify: `frontend/rsbuild.config.ts`
- Modify: `frontend/tsconfig.json`
- Create: `frontend/src/main.tsx`, `frontend/src/styles.css`, `frontend/src/routes/__root.tsx`, `frontend/src/routes/index.tsx`
- Delete: `frontend/src/index.tsx`, `frontend/src/LocoSplash.tsx`, `frontend/src/index.css`

**Interfaces:**
- Consumes: nothing.
- Produces: a working router. Later tasks add files under `src/routes/` and the plugin regenerates `src/routeTree.gen.ts`. `src/styles.css` exposes the CSS custom properties every later task uses (`var(--acc)` etc.) and the classes `.rowHover`, `.btnGhost`, `.linkPlain`.

- [ ] **Step 1: Install deps**

```bash
cd frontend && pnpm add @tanstack/react-router && pnpm add -D @tanstack/router-plugin vitest
```

- [ ] **Step 2: Wire the router plugin into rsbuild**

`frontend/rsbuild.config.ts` — keep the existing `server.proxy` block untouched:

```ts
import { defineConfig } from "@rsbuild/core";
import { pluginReact } from "@rsbuild/plugin-react";
import { tanstackRouter } from "@tanstack/router-plugin/rspack";

export default defineConfig({
  plugins: [pluginReact()],
  tools: {
    rspack: {
      plugins: [tanstackRouter({ target: "react", autoCodeSplitting: true })],
    },
  },
  html: {
    favicon: "src/assets/favicon.ico",
    title: "Object Storage Gate",
  },
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:5150",
        changeOrigin: true,
        secure: false,
      },
    },
  },
});
```

If `@tanstack/router-plugin/rspack` fails to load, stop and report — do not silently fall back to code-based routes.

- [ ] **Step 3: Add the token stylesheet**

`frontend/src/styles.css` — tokens copied verbatim from the `:root` block of `Dashboard.dc.html`:

```css
@import url("https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&family=IBM+Plex+Sans:wght@400;500;600;700&display=swap");

:root {
  --bg: #100f0e; --panel: #181614; --panel2: #1f1d1a; --hover: #26231f;
  --line: #2b2825; --line2: #3a3630;
  --tx: #f2eee8; --dim: #a49d93; --faint: #6f6860;
  --acc: #f97316; --acc2: #ffa352; --accTx: #1a0d03;
  --accSoft: rgba(249, 115, 22, 0.13); --accLine: rgba(249, 115, 22, 0.38);
  --ok: #3fb579; --okSoft: rgba(63, 181, 121, 0.13);
  --warn: #d6c043; --warnSoft: rgba(214, 192, 67, 0.13);
  --dgr: #e8525e; --dgrSoft: rgba(232, 82, 94, 0.13);
  --info: #5b8def; --infoSoft: rgba(91, 141, 239, 0.13);
}

* { box-sizing: border-box; }
body {
  margin: 0; background: var(--bg); color: var(--tx);
  font-family: "IBM Plex Sans", system-ui, sans-serif;
  -webkit-font-smoothing: antialiased;
}
a { color: var(--acc); text-decoration: none; }
a:hover { color: var(--acc2); }
input, select, button, textarea { font-family: inherit; }
input::placeholder { color: var(--faint); }
::selection { background: var(--accSoft); }
@keyframes shim { 0% { background-position: -500px 0; } 100% { background-position: 500px 0; } }
@keyframes spin { to { transform: rotate(360deg); } }

/* prototype style-hover / style-focus, which inline styles cannot express */
.rowHover:hover { background: var(--hover); }
.btnGhost:hover { color: var(--tx); border-color: var(--faint); }
.btnDanger:hover { color: var(--dgr); border-color: var(--dgr); }
.linkPlain { color: inherit; }
.linkPlain:hover { color: inherit; }
input:focus, select:focus, textarea:focus { border-color: var(--acc); }

@media (max-width: 1279px) {
  [data-grid="stats"] { grid-template-columns: 1fr 1fr !important; }
  [data-grid="two"] { grid-template-columns: 1fr !important; }
}
```

- [ ] **Step 4: Root route + entry**

`frontend/src/routes/__root.tsx`:

```tsx
import { Outlet, createRootRoute } from "@tanstack/react-router";

export const Route = createRootRoute({ component: () => <Outlet /> });
```

`frontend/src/routes/index.tsx` (temporary — Task 6 replaces it with the dashboard):

```tsx
import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/")({ component: () => <div>OSG</div> });
```

`frontend/src/main.tsx`:

```tsx
import { RouterProvider, createRouter } from "@tanstack/react-router";
import React from "react";
import ReactDOM from "react-dom/client";
import { routeTree } from "./routeTree.gen";
import "./styles.css";

const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const root = document.getElementById("root");
if (!root) throw new Error("No root element found");

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
);
```

- [ ] **Step 5: Delete the starter, ignore the generated tree**

```bash
cd frontend && rm src/index.tsx src/LocoSplash.tsx src/index.css
printf 'src/routeTree.gen.ts\n' >> .gitignore
```

Add `"test": "vitest run"` to `package.json` scripts. In `tsconfig.json`, add `"types": ["@rsbuild/core/types"]` is already handled by `src/env.d.ts` — leave it; only add `"jsx": "react-jsx"` if missing (it is present).

- [ ] **Step 6: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

Expected: build succeeds, `src/routeTree.gen.ts` generated, no type or lint errors. Then `pnpm dev` and confirm `http://localhost:3000/` renders "OSG" on the dark background with IBM Plex loaded.

- [ ] **Step 7: Stage (do not commit)**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 2: `lib/format.ts` with tests

**Files:**
- Create: `frontend/src/lib/format.ts`, `frontend/src/lib/format.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces:

```ts
export function fmt(n: number): string;
export function grp(n: number): string;
export function colorFor(pct: number): string;
export type QuotaView = {
  unlimited: boolean; usedText: string; maxText: string; pctText: string;
  barW: string; resW: string; color: string; state: string;
};
export function quotaView(used: number, max: number, res?: number): QuotaView;
export type KeyStatus = "active" | "disabled" | "expired" | "revoked";
export type PillView = { pill: string; pillBg: string; pillFg: string };
export function pill(status: KeyStatus): PillView;
export function shortId(id: string): string;
```

These are lifted verbatim from the `DCLogic` helpers in `Dashboard.dc.html` (lines 224–245). Every later screen imports them.

- [ ] **Step 1: Write the failing tests**

`frontend/src/lib/format.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { fmt, grp, pill, quotaView, shortId } from "./format";

describe("fmt", () => {
  it("returns 0 B for zero", () => expect(fmt(0)).toBe("0 B"));
  it("keeps bytes integral", () => expect(fmt(512)).toBe("512 B"));
  it("drops a trailing .0", () => expect(fmt(1024)).toBe("1 KiB"));
  it("uses one decimal above KiB", () => expect(fmt(1536)).toBe("1.5 KiB"));
  it("stops at TiB", () => expect(fmt(1024 ** 5)).toBe("1024 TiB"));
});

describe("grp", () => {
  it("groups thousands with dots", () => expect(grp(128431)).toBe("128.431"));
});

describe("quotaView", () => {
  it("treats max=0 as unlimited", () => {
    const q = quotaView(12 * 1024 ** 3, 0);
    expect(q.unlimited).toBe(true);
    expect(q.maxText).toBe("Không giới hạn");
    expect(q.barW).toBe("0%");
    expect(q.state).toBe("Không giới hạn");
  });

  it("computes percent, colour and remaining", () => {
    const q = quotaView(50 * 1024 ** 3, 100 * 1024 ** 3);
    expect(q.barW).toBe("50.0%");
    expect(q.color).toBe("var(--ok)");
    expect(q.state).toBe("Còn 50 GiB");
    expect(q.pctText).toBe("50 GiB / 100 GiB (50.0%)");
  });

  it("warns at 75% and above", () => {
    expect(quotaView(80, 100).color).toBe("var(--warn)");
  });

  it("flags nearly-full at 90% and above", () => {
    const q = quotaView(95, 100);
    expect(q.color).toBe("var(--dgr)");
    expect(q.state).toBe("Sắp đầy");
  });

  it("clamps used above max to 100%", () => {
    expect(quotaView(200, 100).barW).toBe("100.0%");
  });

  it("caps the reserved segment at the remaining width", () => {
    const q = quotaView(90, 100, 50);
    expect(q.barW).toBe("90.0%");
    expect(q.resW).toBe("10.0%");
  });
});

describe("pill", () => {
  it("maps every status", () => {
    expect(pill("active").pill).toBe("Đang hoạt động");
    expect(pill("disabled").pill).toBe("Tạm khoá");
    expect(pill("expired").pill).toBe("Hết hạn");
    expect(pill("revoked").pill).toBe("Đã thu hồi");
  });
});

describe("shortId", () => {
  it("keeps 7 leading and 4 trailing chars", () => {
    expect(shortId("OSG3f7a91d0c4b29b2c")).toBe("OSG3f7a…9b2c");
  });
});
```

- [ ] **Step 2: Run and watch it fail**

```bash
cd frontend && pnpm test
```

Expected: FAIL — cannot resolve `./format`.

- [ ] **Step 3: Implement**

`frontend/src/lib/format.ts` — logic copied from `Dashboard.dc.html`, typed:

```ts
const UNITS = ["B", "KiB", "MiB", "GiB", "TiB"];

export function fmt(n: number): string {
  if (!n) return "0 B";
  let i = 0;
  let v = n;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const text = i === 0 ? String(Math.round(v)) : v.toFixed(1).replace(/\.0$/, "");
  return `${text} ${UNITS[i]}`;
}

export function grp(n: number): string {
  return n.toLocaleString("en-US").replace(/,/g, ".");
}

export function colorFor(pct: number): string {
  return pct >= 90 ? "var(--dgr)" : pct >= 75 ? "var(--warn)" : "var(--ok)";
}

export type QuotaView = {
  unlimited: boolean;
  usedText: string;
  maxText: string;
  pctText: string;
  barW: string;
  resW: string;
  color: string;
  state: string;
};

export function quotaView(used: number, max: number, res = 0): QuotaView {
  if (!max) {
    return {
      unlimited: true,
      usedText: fmt(used),
      maxText: "Không giới hạn",
      pctText: `${fmt(used)} đã dùng · ∞ Không giới hạn`,
      barW: "0%",
      resW: "0%",
      color: "var(--acc)",
      state: "Không giới hạn",
    };
  }
  const pct = Math.min(100, (used / max) * 100);
  const rp = Math.min(100 - pct, (res / max) * 100);
  return {
    unlimited: false,
    usedText: fmt(used),
    maxText: fmt(max),
    pctText: `${fmt(used)} / ${fmt(max)} (${pct.toFixed(1)}%)`,
    barW: `${pct.toFixed(1)}%`,
    resW: `${rp.toFixed(1)}%`,
    color: colorFor(pct),
    state: pct >= 90 ? "Sắp đầy" : `Còn ${fmt(max - used)}`,
  };
}

export type KeyStatus = "active" | "disabled" | "expired" | "revoked";
export type PillView = { pill: string; pillBg: string; pillFg: string };

export function pill(status: KeyStatus): PillView {
  if (status === "active")
    return { pill: "Đang hoạt động", pillBg: "var(--okSoft)", pillFg: "var(--ok)" };
  if (status === "disabled")
    return { pill: "Tạm khoá", pillBg: "var(--panel2)", pillFg: "var(--dim)" };
  if (status === "expired")
    return { pill: "Hết hạn", pillBg: "var(--accSoft)", pillFg: "var(--acc)" };
  return { pill: "Đã thu hồi", pillBg: "var(--dgrSoft)", pillFg: "var(--dgr)" };
}

export function shortId(id: string): string {
  return `${id.slice(0, 7)}…${id.slice(-4)}`;
}
```

- [ ] **Step 4: Run tests, then the gate**

```bash
cd frontend && pnpm test && pnpm exec tsc --noEmit && pnpm biome check src/
```

Expected: all tests PASS. If a case fails because the prototype's own formula disagrees with the assertion, the **prototype wins** — fix the test, note it in the report.

- [ ] **Step 5: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 3: `lib/mock.ts` fixtures

**Files:**
- Create: `frontend/src/lib/mock.ts`

**Interfaces:**
- Consumes: `KeyStatus` from `./format`.
- Produces:

```ts
export type Bucket = { name: string; used: number; max: number; res: number; objects: number; created: string };
export type AccessKey = { id: string; label: string; status: KeyStatus; created: string; lastUsed: string | null; expiresAt: string | null; permissions: string[]; prefixes: string[]; buckets: string[] };
export type S3Object = { key: string; size: number; contentType: string; etag: string; modified: string };
export type AdminUser = { pid: string; name: string; email: string; role: "user" | "admin"; used: number; maxBytes: number; buckets: number; keys: number; verified: boolean; created: string };
export type SystemStats = { users: number; buckets: number; objects: number; used: number; capacity: number };

export const BUCKETS: Bucket[];
export const KEYS: AccessKey[];
export const OBJECTS: Record<string, S3Object[]>;   // bucket name → objects
export const ADMIN_USERS: AdminUser[];
export const SYSTEM: SystemStats;
export const ENDPOINT = "https://s3.osgate.vn";
export const REGION = "ap-southeast-1";
```

- [ ] **Step 1: Read the fixture data out of the prototypes**

Read the `BUCKETS()`, `KEYS()`, `OBJECTS()`, `USERS()` (names vary) methods in `Dashboard.dc.html`, `Buckets.dc.html`, `Bucket Detail.dc.html`, `Access Keys.dc.html`, `Key Detail.dc.html`, `Admin.dc.html`, `Admin Users.dc.html`, `Admin User Detail.dc.html`, `Admin Buckets.dc.html`. Also read `docs/ui/admin-ui-spec.md` §"type Bucket / type S3Object / type AccessKey / type User" for the field names the real API will use.

- [ ] **Step 2: Write the fixtures**

Union of the per-prototype fixtures into one module — every value copied verbatim (e.g. `{ name: "media-cdn", used: 42.7 * G, max: 50 * G, res: 0, objects: 128431 }`). Define `const G = 1024 ** 3` and `const M = 1024 ** 2` at the top. Where two prototypes disagree on a row, keep the richer one and make the other screen derive what it needs.

- [ ] **Step 3: Verify**

```bash
cd frontend && pnpm exec tsc --noEmit && pnpm biome check src/
```

Expected: clean. No screen imports this yet.

- [ ] **Step 4: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 4: `lib/auth.ts` + `role` on `/api/auth/current`

**Files:**
- Create: `frontend/src/lib/auth.ts`, `frontend/src/lib/auth.test.ts`
- Modify: `src/views/auth.rs:26-40` (`CurrentResponse`)
- Modify: `tests/requests/snapshots/*current*.snap` if a snapshot covers `/current`

**Interfaces:**
- Consumes: nothing.
- Produces:

```ts
export type CurrentUser = { pid: string; name: string; email: string; role: "user" | "admin"; max_bytes: number };
export function getToken(): string | null;
export function setToken(token: string): void;
export function clearToken(): void;
export function api<T>(path: string, init?: RequestInit): Promise<T>;   // throws ApiError
export class ApiError extends Error { status: number }
export function login(email: string, password: string): Promise<{ token: string; pid: string; name: string; is_verified: boolean }>;
export function register(name: string, email: string, password: string): Promise<void>;
export function forgot(email: string): Promise<void>;
export function reset(token: string, password: string): Promise<void>;
export function magicLink(email: string): Promise<void>;
export function current(): Promise<CurrentUser>;
```

Endpoints (verified in `src/controllers/auth.rs:261-273`, prefix `/api/auth`): `POST /register {name,email,password}`, `GET /verify/{token}`, `POST /login {email,password}` → `{token,pid,name,is_verified}`, `POST /forgot {email}`, `POST /reset {token,password}`, `GET /current`, `POST /magic-link {email}`, `GET /magic-link/{token}`.

- [ ] **Step 1: Add `role` and `max_bytes` to the current-user response**

`src/views/auth.rs` — the console needs the role to decide whether to show the ADMIN nav group:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct CurrentResponse {
    pub pid: String,
    pub name: String,
    pub email: String,
    pub role: String,
    pub max_bytes: i64,
}

impl CurrentResponse {
    #[must_use]
    pub fn new(user: &users::Model) -> Self {
        Self {
            pid: user.pid.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
            role: user.role.clone(),
            max_bytes: user.max_bytes,
        }
    }
}
```

- [ ] **Step 2: Run the Rust suite and refresh snapshots**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && cargo test --test requests
```

If an insta snapshot fails on the new fields, run `cargo insta review`, confirm the diff adds only `role` and `max_bytes`, and accept.

- [ ] **Step 3: Write the failing frontend tests**

`frontend/src/lib/auth.test.ts`:

```ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, api, clearToken, getToken, setToken } from "./auth";

afterEach(() => {
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("token storage", () => {
  it("round-trips a token", () => {
    expect(getToken()).toBeNull();
    setToken("abc");
    expect(getToken()).toBe("abc");
    clearToken();
    expect(getToken()).toBeNull();
  });
});

describe("api", () => {
  it("omits Authorization when there is no token", async () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await api("/api/auth/current");
    const headers = new Headers(fetchMock.mock.calls[0][1]?.headers);
    expect(headers.has("Authorization")).toBe(false);
  });

  it("attaches the bearer token when present", async () => {
    setToken("abc");
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await api("/api/auth/current");
    const headers = new Headers(fetchMock.mock.calls[0][1]?.headers);
    expect(headers.get("Authorization")).toBe("Bearer abc");
  });

  it("throws ApiError carrying the status", async () => {
    vi.stubGlobal("fetch", async () => new Response("nope", { status: 401 }));
    await expect(api("/api/auth/current")).rejects.toMatchObject({ status: 401 });
  });

  it("clears the token on 401", async () => {
    setToken("abc");
    vi.stubGlobal("fetch", async () => new Response("nope", { status: 401 }));
    await expect(api("/api/auth/current")).rejects.toBeInstanceOf(ApiError);
    expect(getToken()).toBeNull();
  });

  it("resolves undefined for an empty 204 body", async () => {
    vi.stubGlobal("fetch", async () => new Response(null, { status: 204 }));
    await expect(api("/api/auth/forgot")).resolves.toBeUndefined();
  });
});
```

`localStorage` needs a DOM environment — add `frontend/vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({ test: { environment: "jsdom" } });
```

and `pnpm add -D jsdom`.

- [ ] **Step 4: Run and watch it fail**

```bash
cd frontend && pnpm test
```

Expected: FAIL — cannot resolve `./auth`.

- [ ] **Step 5: Implement**

`frontend/src/lib/auth.ts`:

```ts
const TOKEN_KEY = "osg_token";

export type CurrentUser = {
  pid: string;
  name: string;
  email: string;
  role: "user" | "admin";
  max_bytes: number;
};

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
    this.name = "ApiError";
  }
}

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  const token = getToken();
  if (token) headers.set("Authorization", `Bearer ${token}`);

  const res = await fetch(path, { ...init, headers });
  if (!res.ok) {
    if (res.status === 401) clearToken();
    throw new ApiError(res.status, (await res.text()) || res.statusText);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

const post = <T>(path: string, body: unknown) =>
  api<T>(path, { method: "POST", body: JSON.stringify(body) });

export const login = (email: string, password: string) =>
  post<{ token: string; pid: string; name: string; is_verified: boolean }>(
    "/api/auth/login",
    { email, password },
  );

export const register = (name: string, email: string, password: string) =>
  post<void>("/api/auth/register", { name, email, password });

export const forgot = (email: string) => post<void>("/api/auth/forgot", { email });

export const reset = (token: string, password: string) =>
  post<void>("/api/auth/reset", { token, password });

export const magicLink = (email: string) =>
  post<void>("/api/auth/magic-link", { email });

export const current = () => api<CurrentUser>("/api/auth/current");
```

- [ ] **Step 6: Run tests + gate**

```bash
cd frontend && pnpm test && pnpm exec tsc --noEmit && pnpm biome check src/
```

Expected: all PASS, clean.

- [ ] **Step 7: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend src/views/auth.rs tests
```

---

### Task 5: Auth screens

**Files:**
- Read first: `console-object-storage-gate/project/Object Storage Gate.dc.html` (in full — it holds every auth branch behind `sc-if` on `authLogin`, `authReg`, `authForgot`, `authReset`, `authVerify`)
- Create: `frontend/src/routes/_auth.tsx`, `frontend/src/routes/_auth/login.tsx`, `register.tsx`, `forgot.tsx`, `reset.tsx`, `verify.$token.tsx`, `magic-link.tsx`

**Interfaces:**
- Consumes: `login`, `register`, `forgot`, `reset`, `magicLink`, `setToken`, `ApiError` from `../lib/auth`.
- Produces: routes `/login`, `/register`, `/forgot`, `/reset` (search param `token`), `/verify/$token`, `/magic-link`. Task 6's guard redirects to `/login`.

- [ ] **Step 1: Port the shell**

`_auth.tsx` is the `data-auth="wrap"` container from the prototype: `min-height:100vh; display:grid; place-items:center; background:var(--bg); padding:32px 20px`, inner `width:100%; max-width:360px`, the logo lockup (24px orange rounded square + 9px inner square + "Object Storage Gate" at 14px/600), then `<Outlet/>`.

- [ ] **Step 2: Port each branch to its own route file**

One prototype `sc-if` branch per file. Keep every field, label, helper text, error banner (`--dgrSoft` background, `1px solid rgba(232,82,94,.4)`, `#FF9AA2` text), the password reveal button with its `{{ pwToggleLabel }}` states, and the footer links. Forms submit on Enter (the prototype wires `onKeyDown`).

- [ ] **Step 3: Wire the real calls**

- `/login`: `login()` → `setToken(res.token)` → `navigate({ to: "/" })`. On `ApiError`, show the prototype's error banner; use the prototype's Vietnamese copy for a 401.
- `/register`: `register()` → success state telling the user to check their email (prototype has this branch).
- `/forgot`: `forgot()` → success state. Loco returns 200 regardless of whether the email exists; keep it that way and do not report whether the address was found.
- `/reset`: read `token` from the search params via `validateSearch`, call `reset()`, then navigate to `/login`.
- `/verify/$token`: call `GET /api/auth/verify/{token}` on mount, render the prototype's result states (success / invalid).
- `/magic-link`: `magicLink()` → "check your email" state.

Disable the submit button while a request is in flight, using the prototype's disabled styling.

- [ ] **Step 4: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

Then, with `cargo loco start` running: `pnpm dev`, register a user, verify via the mailer link (or `cargo loco task`), log in, and confirm the token lands in `localStorage` under `osg_token`. Compare each screen against the prototype branch it came from.

- [ ] **Step 5: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 6: App shell — Sidebar, Header, guard, Toast, Modal

**Files:**
- Read first: `console-object-storage-gate/project/Sidebar.dc.html` (in full) and the `<header>` + avatar-menu + logout-modal + toast blocks of `Dashboard.dc.html` (lines 44–62, 200–218)
- Create: `frontend/src/routes/_app.tsx`, `frontend/src/components/Sidebar.tsx`, `Header.tsx`, `Toast.tsx`, `Modal.tsx`, `QuotaBar.tsx`, `Pill.tsx`, `Copyable.tsx`

**Interfaces:**
- Consumes: `getToken`, `clearToken`, `current`, `CurrentUser` from `../lib/auth`; `QuotaView`, `PillView` from `../lib/format`.
- Produces:

```tsx
// components/Sidebar.tsx
export function Sidebar(props: { active: "dash" | "buckets" | "keys" | "admin" | "users" | "abuckets" | "none"; isAdmin: boolean }): JSX.Element;
// components/Header.tsx
export function Header(props: { title: string; user: CurrentUser; right?: React.ReactNode }): JSX.Element;
// components/Toast.tsx
export function ToastProvider(props: { children: React.ReactNode }): JSX.Element;
export function useToast(): (msg: string) => void;
// components/Modal.tsx
export function Modal(props: { width?: number; onClose: () => void; children: React.ReactNode }): JSX.Element;
// components/QuotaBar.tsx
export function QuotaBar(props: { q: QuotaView; height?: number }): JSX.Element;
// components/Pill.tsx
export function Pill(props: { view: PillView }): JSX.Element;
// components/Copyable.tsx
export function Copyable(props: { value: string; label?: string; style?: React.CSSProperties }): JSX.Element;
// routes/_app.tsx
export const Route; // loader provides { user: CurrentUser } to children via useRouteContext / Route.useLoaderData
```

The `_app` route also exports the active-nav key per child via each child calling `<Sidebar active="…">`? No — the sidebar lives in `_app.tsx`; it derives `active` from `useRouterState().location.pathname` (`/` → `dash`, `/buckets*` → `buckets`, `/keys*` → `keys`, `/admin` → `admin`, `/admin/users*` → `users`, `/admin/buckets` → `abuckets`).

- [ ] **Step 1: Port the sidebar**

Verbatim from `Sidebar.dc.html`: 240px expanded / 64px collapsed with `transition:width .12s ease`, 56px brand row, `STORAGE` and `ADMIN` group labels at 10px/.14em, 36px nav rows, each row's inline SVG icon copied exactly, active row `background:var(--accSoft); color:var(--acc)`. Collapse state persists in `localStorage` under `osg_collapsed` (`"1"`/`"0"`). Footer holds settings / logout / collapse buttons; direction flips to `column` when collapsed. Hide the ADMIN group when `isAdmin` is false.

- [ ] **Step 2: Port the header, avatar menu, logout confirm and toast**

Header: 56px, `background:rgba(16,15,14,.86)`, `backdrop-filter:blur(8px)`, `position:sticky; top:0; z-index:30`. Avatar: 30px circle with the user's initials, `--accSoft` fill and `--accLine` border. Menu: 200px panel with email, role label, Hồ sơ / Cài đặt links and a `--dgr` logout item; a `position:fixed; inset:0` click-catcher closes it. Logout modal and toast come from `Dashboard.dc.html` lines 200–218 — put them in `Modal.tsx` / `Toast.tsx` so every screen reuses them.

- [ ] **Step 3: Guard the layout**

```tsx
// routes/_app.tsx
beforeLoad: () => {
  if (!getToken()) throw redirect({ to: "/login" });
},
loader: () => current(),
```

Render `{ display:flex; min-height:100vh; background:var(--bg) }` → `<Sidebar/>` + `<main>` with `<Header/>` and `<Outlet/>`. Content wrapper: `flex:1; padding:24px; max-width:1400px; width:100%; margin:0 auto`. Logout clears the token and navigates to `/login`. This guard is UX only — server-side enforcement lands with slice #7.

- [ ] **Step 4: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

Then in `pnpm dev`: visiting `/` without a token redirects to `/login`; after logging in the shell renders; collapsing the sidebar survives a reload; the avatar menu opens and closes; logout returns to `/login`. Compare the sidebar against `Sidebar.dc.html` at both widths.

- [ ] **Step 5: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 7: Dashboard

**Files:**
- Read first: `console-object-storage-gate/project/Dashboard.dc.html` (in full)
- Modify: `frontend/src/routes/index.tsx` → delete; Create: `frontend/src/routes/_app/index.tsx`

**Interfaces:**
- Consumes: `fmt`, `grp`, `quotaView`, `pill`, `shortId` from `../../lib/format`; `BUCKETS`, `KEYS`, `SYSTEM`, `ENDPOINT`, `REGION` from `../../lib/mock`; `QuotaBar`, `Pill`, `Copyable`, `useToast`.
- Produces: route `/`.

- [ ] **Step 1: Port the empty state**

The three numbered onboarding cards (`Chào mừng tới Object Storage Gate`), shown when the user owns no buckets. Card 1 is active (`--accLine` border, orange CTA); cards 2 and 3 are `opacity:.72`.

- [ ] **Step 2: Port the stat row**

`data-grid="stats"`, `grid-template-columns:1.5fr 1fr 1fr 1fr; gap:14px`. Card 1 is the account quota: 28px IBM Plex Mono figure, the two-segment bar (solid used + 45° striped reserved at `opacity:.6`), percent text and state. Cards 2–4 are bucket / object / active-key counts with their sub-labels. Keep the `@keyframes shim` skeleton block for the loading branch.

- [ ] **Step 3: Port the two panels and the snippet block**

`data-grid="two"`, `1.35fr 1fr`. "Bucket dùng nhiều nhất": top 4 buckets as `minmax(110px,180px) 1fr auto` rows linking to `/buckets/$name`, each with a 5px bar (only when limited) and an object count. "Access key gần nhất": 3 rows linking to `/keys/$pid` with a status pill. Then "Kết nối nhanh": aws-cli / rclone / boto3 tabs over a `<pre>` on `#0C0B0A`, plus the copy button — snippet strings copied verbatim from `renderVals()`.

- [ ] **Step 4: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`: `/` renders every block; the tabs switch snippets; copy raises the toast; narrowing below 1280px collapses both grids. Compare against `Dashboard.dc.html`.

- [ ] **Step 5: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 8: Buckets list

**Files:**
- Read first: `console-object-storage-gate/project/Buckets.dc.html` (in full)
- Create: `frontend/src/routes/_app/buckets/index.tsx`

**Interfaces:**
- Consumes: `BUCKETS` from `../../../lib/mock`; `fmt`, `grp`, `quotaView`; `Modal`, `QuotaBar`, `useToast`.
- Produces: route `/buckets`. Rows link to `/buckets/$name`; the gear links to `/buckets/$name/settings`.

- [ ] **Step 1: Port the table**

Header row, sort controls, search box, per-row bucket name in IBM Plex Mono, quota bar, object count, created date, row actions — every column width, padding and colour from the prototype. Search filters client-side over the mock list, exactly as the prototype's `query` state does.

- [ ] **Step 2: Port the create-bucket flow**

The prototype's create form (modal or inline panel — follow the prototype) with S3 name validation (lowercase letters, digits, hyphens; 3–63 chars; no leading/trailing hyphen) and its inline error copy, plus the optional quota field where empty means unlimited. On submit, push onto local state and raise the prototype's success toast — no API call exists yet; leave `// TODO(slice#7): POST /api/buckets`.

- [ ] **Step 3: Port the empty state** as the prototype defines it.

- [ ] **Step 4: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`: table renders, search filters, sort reorders, invalid names are rejected with the prototype's message, a valid name appears in the list. Compare against `Buckets.dc.html`.

- [ ] **Step 5: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 9: Bucket detail (object browser)

**Files:**
- Read first: `console-object-storage-gate/project/Bucket Detail.dc.html` (in full)
- Create: `frontend/src/routes/_app/buckets/$name.tsx`

**Interfaces:**
- Consumes: `BUCKETS`, `OBJECTS` from `../../../lib/mock`; `fmt`, `grp`, `quotaView`; `Copyable`, `Pill`, `Modal`, `useToast`.
- Produces: route `/buckets/$name`. Reads `const { name } = Route.useParams()`.

- [ ] **Step 1: Port the header block** — bucket name, quota summary, the action buttons and the link to `/buckets/$name/settings`.

- [ ] **Step 2: Port the object list** — prefix breadcrumb, folder-vs-object rows, key / size / content-type / ETag / modified columns, selection, per-row actions, pagination or "load more" exactly as the prototype has it. Data comes from `OBJECTS[name]`; a missing bucket renders the prototype's not-found state.

- [ ] **Step 3: Port the delete confirm** — prototype's modal copy and `--dgr` button. Removes from local state only; leave `// TODO(slice#7): DELETE /api/buckets/:name/objects/:key`.

- [ ] **Step 4: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`: `/buckets/media-cdn` lists objects, prefix navigation works, delete confirm removes a row, an unknown bucket name shows the not-found state. Compare against `Bucket Detail.dc.html`.

- [ ] **Step 5: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 10: Bucket settings

**Files:**
- Read first: `console-object-storage-gate/project/Bucket Settings.dc.html` (in full)
- Create: `frontend/src/routes/_app/buckets/$name.settings.tsx`

**Interfaces:**
- Consumes: `BUCKETS`; `fmt`, `quotaView`; `Modal`, `useToast`.
- Produces: route `/buckets/$name/settings`.

- [ ] **Step 1: Port the quota form** — current usage, the max-bytes input with its unit selector, the "0 / empty = unlimited" helper text, and the guard against setting a quota below current usage with the prototype's error copy.

- [ ] **Step 2: Port the danger zone** — delete-bucket panel with the type-the-name confirmation the prototype requires. Leave `// TODO(slice#7): DELETE /api/buckets/:name`.

- [ ] **Step 3: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`: saving raises the toast; a quota below usage is rejected; delete stays disabled until the name matches. Compare against `Bucket Settings.dc.html`.

- [ ] **Step 4: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 11: Access keys list

**Files:**
- Read first: `console-object-storage-gate/project/Access Keys.dc.html` (in full)
- Create: `frontend/src/routes/_app/keys/index.tsx`

**Interfaces:**
- Consumes: `KEYS` from `../../../lib/mock`; `pill`, `shortId`; `Pill`, `Modal`, `Copyable`, `useToast`.
- Produces: route `/keys`. Rows link to `/keys/$pid` (the key id).

- [ ] **Step 1: Port the table** — key id in mono, label, status pill, created / last-used / expiry columns, status filter, and the row actions (disable, revoke) with their confirm modals.

- [ ] **Step 2: Port the create-key flow** — permission preset choice (Read-only / Read-write / custom), optional label, optional expiry, then the **secret-shown-once** panel: the prototype's warning copy, the copy button, and the acknowledge action that closes it. The secret must not be recoverable after closing — no state keeps it around. Leave `// TODO(slice#7): POST /api/keys`.

- [ ] **Step 3: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`: table renders with correct pills, the filter narrows rows, creating a key shows the secret panel once and never again, revoke asks for confirmation. Compare against `Access Keys.dc.html`.

- [ ] **Step 4: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 12: Key detail (permissions + prefixes)

**Files:**
- Read first: `console-object-storage-gate/project/Key Detail.dc.html` (in full)
- Create: `frontend/src/routes/_app/keys/$pid.tsx`

**Interfaces:**
- Consumes: `KEYS`; `pill`, `shortId`; `Pill`, `Modal`, `Copyable`, `useToast`.
- Produces: route `/keys/$pid`.

- [ ] **Step 1: Port the summary header** — key id with copy, status pill, label, created / last-used / expiry, and the disable / revoke / rotate actions with their confirms.

- [ ] **Step 2: Port the permission editor** — the S3 verb checkboxes/toggles exactly as grouped in the prototype, with the preset shortcuts.

- [ ] **Step 3: Port the prefix editor** — add / remove prefix rows, the validation the prototype applies, and the empty state meaning "whole bucket". Leave `// TODO(slice#7): PATCH /api/keys/:pid`.

- [ ] **Step 4: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`: `/keys/OSG3f7a91d0c4b29b2c` renders, toggling a permission marks the form dirty, saving raises the toast, an unknown id shows the not-found state. Compare against `Key Detail.dc.html`.

- [ ] **Step 5: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 13: Settings + Profile

**Files:**
- Read first: `console-object-storage-gate/project/Settings.dc.html` and `Profile.dc.html` (both in full)
- Create: `frontend/src/routes/_app/settings.tsx`, `frontend/src/routes/_app/profile.tsx`

**Interfaces:**
- Consumes: `CurrentUser` from the `_app` loader; `fmt`, `quotaView`; `Copyable`, `useToast`.
- Produces: routes `/settings`, `/profile`.

- [ ] **Step 1: Port Profile** — name / email / role / joined, the account quota summary, and the change-password form with the prototype's validation copy. No password endpoint exists in the starter; leave `// TODO: password change endpoint` and keep the form inert.

- [ ] **Step 2: Port Settings** — endpoint / region display with copy buttons and whatever preference controls the prototype defines.

- [ ] **Step 3: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`: both routes render, the real logged-in user's name/email/role appear, copy buttons raise the toast. Compare against the two prototypes.

- [ ] **Step 4: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 14: Admin dashboard

**Files:**
- Read first: `console-object-storage-gate/project/Admin.dc.html` (in full)
- Create: `frontend/src/routes/_app/admin/index.tsx`

**Interfaces:**
- Consumes: `SYSTEM`, `ADMIN_USERS`, `BUCKETS`; `fmt`, `grp`, `quotaView`; `QuotaBar`.
- Produces: route `/admin`, plus the admin guard pattern the next two tasks reuse:

```tsx
beforeLoad: ({ context }) => {
  // user comes from the _app loader
  if (context.user.role !== "admin") throw redirect({ to: "/" });
},
```

Wire `_app.tsx` to expose its loader data as route context so this check works; if that turns out awkward in the installed router version, read the role inside the component and render `<Navigate to="/" />` instead — say which you did in the report.

- [ ] **Step 1: Port the system stat cards and panels** — user / bucket / object / capacity figures and the leaderboard tables, verbatim.

- [ ] **Step 2: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev`, logged in as an admin (`UPDATE users SET role='admin' WHERE email=…` in the dev DB): `/admin` renders and the ADMIN nav group is visible. Logged in as a plain user: `/admin` redirects to `/` and the group is hidden. Compare against `Admin.dc.html`.

- [ ] **Step 3: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 15: Admin users list + detail

**Files:**
- Read first: `console-object-storage-gate/project/Admin Users.dc.html` and `Admin User Detail.dc.html` (both in full)
- Create: `frontend/src/routes/_app/admin/users.index.tsx`, `frontend/src/routes/_app/admin/users.$pid.tsx`

**Interfaces:**
- Consumes: `ADMIN_USERS`, `BUCKETS`, `KEYS`; `fmt`, `grp`, `quotaView`, `pill`; `Pill`, `QuotaBar`, `Modal`, `useToast`. Same admin guard as Task 14.
- Produces: routes `/admin/users`, `/admin/users/$pid`.

- [ ] **Step 1: Port the users table** — email / name / role / usage bar / bucket & key counts / verified state, with search and role filter.

- [ ] **Step 2: Port the user detail screen** — profile block, quota form (grant / change `max_bytes`), role switch with its confirm, and the read-only lists of that user's buckets and keys. Per `docs/ui/admin-ui-spec.md` §2 there is **no** delete-other-users'-objects action — do not add one. Leave `// TODO(slice#7): PATCH /api/admin/users/:pid`.

- [ ] **Step 3: Verify**

```bash
cd frontend && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
```

In `pnpm dev` as admin: the list renders and filters; a row opens the detail; the quota form validates; the role switch asks for confirmation. Compare against both prototypes.

- [ ] **Step 4: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend
```

---

### Task 16: Admin buckets (Pool) + final sweep

**Files:**
- Read first: `console-object-storage-gate/project/Admin Buckets.dc.html` (in full)
- Create: `frontend/src/routes/_app/admin/buckets.tsx`
- Modify: `frontend/README.md`

**Interfaces:**
- Consumes: `BUCKETS`, `ADMIN_USERS`, `SYSTEM`; `fmt`, `grp`, `quotaView`; `QuotaBar`. Same admin guard as Task 14.
- Produces: route `/admin/buckets`.

- [ ] **Step 1: Port the pool screen** — every bucket across all users with its owner, usage bar and object count, plus the physical-bucket summary the prototype shows.

- [ ] **Step 2: Sweep**

- Every prototype in `console-object-storage-gate/project/` has a route (13 files, `support.js` and `.thumbnail` excluded).
- No `.dc.html`, `{{ }}`, `sc-if`, `sc-for` or `dc-import` remnants: `grep -rE 'dc\.html|sc-if|sc-for|dc-import|\{\{' frontend/src` returns nothing.
- No leftover `LocoSplash`, `index.css` or `index.tsx` references.
- Every `// TODO(slice#7)` marker names the endpoint it is waiting on.
- Rewrite `frontend/README.md`: scripts, route map, where the design source lives, and the fact that non-auth data is mocked until slice #7.

- [ ] **Step 3: Full verification**

```bash
cd frontend && pnpm test && pnpm build && pnpm exec tsc --noEmit && pnpm biome check src/
cd /Users/op-lt-0366/hdg/object-storage-gate && cargo test
```

Then walk all 15 routes in `pnpm dev` with the browser console open: no errors, no warnings about missing `key` props, no 404s on the `/api` proxy.

- [ ] **Step 4: Stage**

```bash
cd /Users/op-lt-0366/hdg/object-storage-gate && git add frontend src tests docs
```

---

## Self-review notes

- Spec coverage: scaffold (T1), `format.ts` (T2), `mock.ts` (T3), `auth.ts` + `CurrentResponse` (T4), auth screens (T5), shell + guard (T6), and the ten console screens (T7–T16) cover every row of the spec's prototype→route map. Verification steps match the spec's Verification section. Out-of-scope items (TanStack Query/Table/Form, real non-auth APIs, light theme, i18n) appear in no task.
- `role` on `/api/auth/current` is the one backend change; it is required for the admin nav gate and lives in T4 with its snapshot refresh.
- Task granularity: T7–T16 are one screen each because a reviewer can accept or reject one screen's fidelity independently.
- Commit steps are `git add` only, per `CLAUDE.md`.
