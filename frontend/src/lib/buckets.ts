import { api } from "./auth";

export type Bucket = {
  pid: string;
  name: string;
  max_bytes: number;
  used_bytes: number;
  reserved_bytes: number;
  object_count: number;
  public_enabled: boolean;
  created_at: string;
};

export const listBuckets = () => api<Bucket[]>("/api/buckets");

export const createBucket = (name: string, max_bytes: number) =>
  api<Bucket>("/api/buckets", {
    method: "POST",
    body: JSON.stringify({ name, max_bytes }),
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
