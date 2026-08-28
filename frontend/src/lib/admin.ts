import { api } from "./auth";

export type AdminUser = {
  pid: string;
  email: string;
  name: string;
  role: "user" | "admin";
  max_bytes: number;
  used_bytes: number;
  reserved_bytes: number;
  must_change_password: boolean;
  created_at: string;
};

export const listUsers = () => api<AdminUser[]>("/api/admin/users");

export const createUser = (body: {
  email: string;
  name: string;
  password: string;
  role: "user" | "admin";
  max_bytes: number;
}) =>
  api<AdminUser>("/api/admin/users", {
    method: "POST",
    body: JSON.stringify(body),
  });

export const getUser = (pid: string) =>
  api<AdminUser>(`/api/admin/users/${pid}`);

export const updateUser = (
  pid: string,
  patch: { name?: string; role?: "user" | "admin"; max_bytes?: number },
) =>
  api<AdminUser>(`/api/admin/users/${pid}`, {
    method: "PATCH",
    body: JSON.stringify(patch),
  });

/** Issues a new temporary password; the user must replace it at next login. */
export const setUserPassword = (pid: string, password: string) =>
  api<void>(`/api/admin/users/${pid}/password`, {
    method: "POST",
    body: JSON.stringify({ password }),
  });

export const deleteUser = (pid: string) =>
  api<void>(`/api/admin/users/${pid}`, { method: "DELETE" });
