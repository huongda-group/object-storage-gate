# Object Storage Gate — console

React SPA for the gateway console. Built with:

- [TanStack Router](https://tanstack.com/router) — file-based routes under `src/routes/`
- [Rsbuild](https://rsbuild.dev/) (rspack) — bundler; the router's rspack plugin generates `src/routeTree.gen.ts` on every build/dev run
- [React 19](https://react.dev/) + TypeScript
- [Biome](https://biomejs.dev/) — lint + format, [Vitest](https://vitest.dev/) — unit tests for `src/lib/`

## Commands

```sh
pnpm install
pnpm dev        # dev server on :3000, proxies /api → http://localhost:5150
pnpm build      # → dist/, served statically by `cargo loco start`
pnpm test       # vitest (src/lib/*.test.ts)
pnpm lint       # biome check src/
pnpm exec tsc --noEmit   # run `pnpm build` first: it regenerates routeTree.gen.ts
```

`pnpm dev --port 3100` if something else already holds :3000.

## Where the design comes from

Every screen is a port of a Claude Design prototype in `../console-object-storage-gate/project/*.dc.html`;
each route file names its source in the first comment. Behaviour, copy and data
shapes follow `../docs/ui/admin-ui-spec.md`. Inline styles are copied verbatim from
the prototypes — only `:hover` / `:focus` / `@media` live in `src/styles.css`,
because inline styles cannot express them.

## Routes

| Route | Screen |
|---|---|
| `/login` `/register` `/forgot` `/reset?token=` `/verify/$token` `/magic-link` | auth (real API) |
| `/` | dashboard |
| `/buckets` `/buckets/$name` `/buckets/$name/settings` | buckets, object browser, quota + delete |
| `/keys` `/keys/$pid` | access keys, permissions + prefixes |
| `/settings` `/profile` | profile, password, account stats |
| `/admin` `/admin/users` `/admin/users/$pid` `/admin/buckets` | admin only (`users.role = 'admin'`) |

## Data

Auth talks to the real loco endpoints (`/api/auth/*`). Everything else reads
fixtures from `src/lib/mock.ts` — the buckets/keys/objects/admin API is slice #7.
Mutations update local state and raise the same toast the prototype does; each
call site carries a `TODO(slice#7)` naming the endpoint it is waiting on.

Route guards (`src/routes/_app.tsx`, and the admin `beforeLoad` checks) are UX
only. The server must enforce both.
