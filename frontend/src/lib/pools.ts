import { api } from "./auth";

export const PROVIDERS = [
  "aws",
  "r2",
  "b2",
  "spaces",
  "minio",
  "ceph",
  "custom",
] as const;

export type Provider = (typeof PROVIDERS)[number];

export type Pool = {
  pid: string;
  name: string;
  provider: Provider;
  region: string | null;
  api_endpoint: string | null;
  physical_bucket: string;
  access_id: string | null;
  is_configured: boolean;
  created_at: string;
};

/** What a non-admin is allowed to see: enough to pick a pool, no credentials, no physical layout. */
export type PoolChoice = {
  pid: string;
  name: string;
  provider: Provider;
};

export type PoolInput = {
  name: string;
  provider: Provider;
  region?: string;
  api_endpoint?: string;
  physical_bucket: string;
  access_id?: string;
  access_secret?: string;
};

export const listPools = () => api<Pool[]>("/api/admin/pools");

/** The listing a bucket-creation form uses. Works for admins and plain users alike. */
export const listPoolChoices = () => api<PoolChoice[]>("/api/pools");

export const createPool = (input: PoolInput) =>
  api<Pool>("/api/admin/pools", {
    method: "POST",
    body: JSON.stringify(input),
  });

export const getPool = (pid: string) => api<Pool>(`/api/admin/pools/${pid}`);

/**
 * Blank fields are dropped, not sent.
 *
 * The server treats an absent `access_secret` as "keep the stored one"; sending an empty
 * string would erase the credential of a pool that is serving traffic.
 */
export const updatePool = (
  pid: string,
  patch: Partial<Omit<PoolInput, "name" | "provider">>,
) => {
  const body: Record<string, string> = {};
  for (const [k, v] of Object.entries(patch)) {
    if (v !== undefined && v !== "") body[k] = v;
  }
  return api<Pool>(`/api/admin/pools/${pid}`, {
    method: "PATCH",
    body: JSON.stringify(body),
  });
};

export const deletePool = (pid: string) =>
  api<void>(`/api/admin/pools/${pid}`, { method: "DELETE" });
