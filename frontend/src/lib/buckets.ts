import { api } from "./auth";

export type Bucket = {
  pid: string;
  name: string;
  max_bytes: number;
  used_bytes: number;
  reserved_bytes: number;
  object_count: number;
  public_enabled: boolean;
  /** The pool's pid, not its row id. */
  pool_id: string;
  pool_name: string;
  created_at: string;
};

export const listBuckets = () => api<Bucket[]>("/api/buckets");

export const createBucket = (
  name: string,
  max_bytes: number,
  pool_id: string,
) =>
  api<Bucket>("/api/buckets", {
    method: "POST",
    body: JSON.stringify({ name, max_bytes, pool_id }),
  });

export const getBucket = (pid: string) => api<Bucket>(`/api/buckets/${pid}`);

export const updateBucket = (
  pid: string,
  patch: { max_bytes?: number; public_enabled?: boolean },
) =>
  api<Bucket>(`/api/buckets/${pid}`, {
    method: "PATCH",
    body: JSON.stringify(patch),
  });

export const deleteBucket = (pid: string) =>
  api<void>(`/api/buckets/${pid}`, { method: "DELETE" });
