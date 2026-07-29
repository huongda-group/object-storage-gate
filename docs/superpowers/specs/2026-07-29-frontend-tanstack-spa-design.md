# Frontend → TanStack Router SPA

**Date:** 2026-07-29
**Scope:** Replace the loco starter frontend (`frontend/`) with a TanStack Router SPA that implements the console designs in `console-object-storage-gate/project/`.

## Context

`frontend/` is still the untouched loco starter: `index.tsx` + `LocoSplash.tsx`, React 18 on rsbuild. Nothing to preserve.

Two inputs already exist:

- `docs/ui/admin-ui-spec.md` — UI spec: roles, sitemap, per-screen behaviour, data shapes.
- `console-object-storage-gate/project/*.dc.html` — 13 Claude Design prototypes. Each file is a template (HTML with inline styles, `{{ }}` bindings, `<sc-if>`, `<sc-for>`, `<dc-import>`) plus a `class Component extends DCLogic` holding state, formatting helpers, and mock data. `support.js` is the design-tool runtime and is **not** ported.

Backend state: only JWT auth endpoints are real. Buckets / access keys / objects / admin APIs land in slice #7.

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Router | `@tanstack/react-router`, file-based routes via `@tanstack/router-plugin/rspack` | File tree ≈ sitemap; avoids ~15 hand-written `createRoute` blocks |
| Bundler | Keep rsbuild | `/api` dev proxy and `frontend/dist` output (served static by the Rust server) stay untouched |
| No TanStack Start | — | Start needs a Node server; the Rust server serves the built assets |
| Styling | Inline styles copied verbatim from the prototypes + one `styles.css` for tokens, fonts, and hover/focus | Pixel-exact port with no translation step; inline styles can't express `:hover`/`:focus` |
| Data layer | Real `fetch` for auth; `lib/mock.ts` for everything else | Only auth exists server-side. No TanStack Query/Table/Form until slice #7 gives them something to do |
| State | Local `useState` per screen (mirrors each prototype's `state`) | No cross-screen state beyond the auth token |

## Structure

```
frontend/src/
  main.tsx                 router mount (replaces index.tsx; LocoSplash.tsx deleted)
  styles.css               :root tokens, IBM Plex, hover/focus classes, @media rules
  lib/format.ts            fmt, grp, colorFor, quotaView, pill, shortId
  lib/mock.ts              buckets, keys, objects, users fixtures
  lib/auth.ts              token in localStorage, api() fetch wrapper, current user
  components/              Sidebar, Header (avatar menu), Toast, Modal, QuotaBar, Pill, Copyable
  routes/
    __root.tsx
    _auth.tsx              centered auth shell
    _auth/login.tsx  register.tsx  forgot.tsx  reset.tsx  verify.$token.tsx  magic-link.tsx
    _app.tsx               sidebar + sticky header + guard
    _app/index.tsx         Dashboard
    _app/buckets/index.tsx  $name.tsx  $name.settings.tsx
    _app/keys/index.tsx  $pid.tsx
    _app/settings.tsx  profile.tsx
    _app/admin/index.tsx  users.index.tsx  users.$pid.tsx  buckets.tsx
```

### Prototype → route map

| Prototype | Route |
|---|---|
| `Object Storage Gate.dc.html` (branches on `authLogin`/`authReg`/…) | `_auth/*` — one file per branch |
| `Sidebar.dc.html` | `components/Sidebar.tsx` (collapse state in `localStorage` key `osg_collapsed`) |
| `Dashboard.dc.html` | `_app/index.tsx` |
| `Buckets.dc.html` | `_app/buckets/index.tsx` |
| `Bucket Detail.dc.html` | `_app/buckets/$name.tsx` |
| `Bucket Settings.dc.html` | `_app/buckets/$name.settings.tsx` |
| `Access Keys.dc.html` | `_app/keys/index.tsx` |
| `Key Detail.dc.html` | `_app/keys/$pid.tsx` |
| `Settings.dc.html` | `_app/settings.tsx` |
| `Profile.dc.html` | `_app/profile.tsx` |
| `Admin.dc.html` | `_app/admin/index.tsx` |
| `Admin Users.dc.html` | `_app/admin/users.index.tsx` |
| `Admin User Detail.dc.html` | `_app/admin/users.$pid.tsx` |
| `Admin Buckets.dc.html` | `_app/admin/buckets.tsx` |

### Port rules

Mechanical, applied per prototype:

- Template → JSX. Inline `style="..."` → `style={{...}}` with the **same values**; never re-invent a number, colour, or spacing.
- `<sc-if value="{{ x }}">` → `{x && (...)}`. `<sc-for list="{{ xs }}" as="x">` → `{xs.map(x => ...)}` with a stable `key`.
- `style-hover` / `style-focus` → a class in `styles.css` (e.g. `.rowHover:hover`, `input:focus`).
- `renderVals()` → derived values in the component body; `state = {...}` → `useState`.
- Prototype `<a href="X.dc.html">` → `<Link to="/…">`.
- Shared helpers (`fmt`, `quotaView`, `pill`, `shortId`) are lifted to `lib/format.ts` once, not copied per screen.
- Demo-only affordances in the prototypes (the `demo` state switching normal/loading/empty/forbidden) are dropped; the real loading/empty/error branches are kept and driven by data.

### Auth

`lib/auth.ts` wraps the existing loco endpoints (`/api/auth/login`, `/register`, `/forgot`, `/reset`, `/verify/:token`, `/magic-link`), stores the JWT in `localStorage`, and exposes `api(path, init)` which attaches `Authorization: Bearer`.

`_app.tsx` `beforeLoad` redirects to `/login` when no token is present. Admin routes additionally check `role === "admin"` and redirect to `/` otherwise. Client-side guards are UX only — real enforcement is server-side and arrives with slice #7's API.

### Mock data

`lib/mock.ts` holds the fixtures the prototypes already define (buckets with `used`/`max`/`res`/`objects`, keys with `id`/`label`/`status`/`created`, admin users, object listings), typed with the shapes from `docs/ui/admin-ui-spec.md` §data so swapping in real API calls at slice #7 is a call-site change only.

## Verification

- `pnpm build` succeeds; `tsc --noEmit` clean; `pnpm biome check` clean.
- Dev server: every route in the map renders, sidebar navigation reaches all of them, no console errors.
- Auth flow against a running `cargo loco start`: register → verify → login → guard admits, logout → guard redirects.
- Side-by-side visual comparison per screen against the prototype file it came from.

## Out of scope

- TanStack Query / Table / Form — added when slice #7 exposes the APIs.
- Real buckets/keys/objects/admin API wiring.
- Light theme, i18n (prototypes are Vietnamese-only), mobile layouts beyond the `@media` rules the prototypes already carry.
