const TOKEN_KEY = "osg_token";

export type CurrentUser = {
  pid: string;
  name: string;
  email: string;
  role: "user" | "admin";
  max_bytes: number;
  must_change_password: boolean;
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
  return localStorage.getItem(TOKEN_KEY) ?? sessionStorage.getItem(TOKEN_KEY);
}

/** `remember` = the login form's "Ghi nhớ thiết bị này": survives a browser restart. */
export function setToken(token: string, remember = true): void {
  clearToken();
  (remember ? localStorage : sessionStorage).setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
  sessionStorage.removeItem(TOKEN_KEY);
}

// beforeLoad runs on every navigation; fetch the user once per session instead.
let currentCache: Promise<CurrentUser> | null = null;

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }
  const token = getToken();
  if (token) headers.set("Authorization", `Bearer ${token}`);

  const res = await fetch(path, { ...init, headers });
  if (!res.ok) {
    // An expired or revoked session used to only clear the token, leaving a fully rendered
    // console where every action failed silently until the next navigation.
    if (res.status === 401) {
      clearToken();
      currentCache = null;
      if (globalThis.location?.pathname !== "/login") {
        globalThis.location?.assign("/login");
      }
    }
    throw new ApiError(res.status, (await res.text()) || res.statusText);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

const post = <T>(path: string, body: unknown) =>
  api<T>(path, { method: "POST", body: JSON.stringify(body) });

export type LoginResponse = {
  token: string;
  pid: string;
  name: string;
  must_change_password: boolean;
};

export const login = (email: string, password: string) =>
  post<LoginResponse>("/api/auth/login", { email, password });

/** True until the very first user exists — the console then sends visitors to /setup. */
export const setupStatus = () =>
  api<{ needs_setup: boolean }>("/api/auth/setup");

export const setupAdmin = (name: string, email: string, password: string) =>
  post<LoginResponse>("/api/auth/setup", { name, email, password });

/** Also the way an admin-issued temporary password gets replaced. */
export const changePassword = (
  current_password: string,
  new_password: string,
) => post<void>("/api/me/password", { current_password, new_password });

export const current = () => api<CurrentUser>("/api/auth/current");

export type Summary = {
  used_bytes: number;
  reserved_bytes: number;
  max_bytes: number;
  bucket_count: number;
  object_count: number;
  active_key_count: number;
};

export const getSummary = () => api<Summary>("/api/me/summary");

export function currentCached(): Promise<CurrentUser> {
  if (!currentCache) {
    currentCache = current().catch((e) => {
      currentCache = null;
      throw e;
    });
  }
  return currentCache;
}

export function logout(): void {
  clearToken();
  currentCache = null;
}
