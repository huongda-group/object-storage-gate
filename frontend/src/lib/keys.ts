import { api } from "./auth";
// KeyStatus already exists for the StatusPill — do not define a second one.
import type { KeyStatus } from "./format";

export type Permission =
  | "read"
  | "write"
  | "delete"
  | "list"
  | "multipart"
  | "presigned";

export type ApiKey = {
  pid: string;
  access_key_id: string;
  label: string;
  status: KeyStatus;
  expires_at: string | null;
  days_until_expiry: number | null;
  permissions: Permission[];
  prefixes: string[];
  created_at: string;
};

export type NewApiKey = ApiKey & { secret: string };

export type CreateKeyInput = {
  label: string;
  permissions: Permission[];
  prefixes: string[];
  expires_at?: string | null;
};

export type UpdateKeyInput = Partial<{
  label: string;
  status: "active" | "disabled";
  expires_at: string | null;
  permissions: Permission[];
  prefixes: string[];
}>;

const json = <T>(path: string, method: string, body?: unknown) =>
  api<T>(path, {
    method,
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });

export const listKeys = () => api<ApiKey[]>("/api/keys");
export const getKey = (pid: string) => api<ApiKey>(`/api/keys/${pid}`);
export const createKey = (input: CreateKeyInput) =>
  json<NewApiKey>("/api/keys", "POST", input);
export const updateKey = (pid: string, patch: UpdateKeyInput) =>
  json<ApiKey>(`/api/keys/${pid}`, "PATCH", patch);
export const rotateKey = (pid: string) =>
  json<NewApiKey>(`/api/keys/${pid}/rotate`, "POST");
export const revokeKey = (pid: string) =>
  json<ApiKey>(`/api/keys/${pid}`, "DELETE");

export const getPat = () => api<{ token: string }>("/api/token");
export const rotatePat = () =>
  json<{ token: string }>("/api/token/rotate", "POST");

/** Calls the API with the PAT itself, not the console session. */
export async function whoami(pat: string) {
  const res = await fetch("/api/whoami", {
    headers: { Authorization: `Bearer ${pat}` },
  });
  return { status: res.status, body: await res.text() };
}

/** Console column copy for `days_until_expiry`. */
export function expiryLabel(daysUntilExpiry: number | null): string | null {
  if (daysUntilExpiry === null) return null;
  if (daysUntilExpiry === 0) return "Hết hạn hôm nay";
  return `Còn ${daysUntilExpiry} ngày`;
}
