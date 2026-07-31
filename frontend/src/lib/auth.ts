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

export type LoginResponse = {
  token: string;
  pid: string;
  name: string;
  is_verified: boolean;
};

export const login = (email: string, password: string) =>
  post<LoginResponse>("/api/auth/login", { email, password });

export const register = (name: string, email: string, password: string) =>
  post<void>("/api/auth/register", { name, email, password });

export const forgot = (email: string) =>
  post<void>("/api/auth/forgot", { email });

export const reset = (token: string, password: string) =>
  post<void>("/api/auth/reset", { token, password });

export const magicLink = (email: string) =>
  post<void>("/api/auth/magic-link", { email });

export const resendVerification = (email: string) =>
  post<void>("/api/auth/resend-verification-mail", { email });

export const verify = (token: string) =>
  api<void>(`/api/auth/verify/${encodeURIComponent(token)}`);

export const current = () => api<CurrentUser>("/api/auth/current");

// beforeLoad runs on every navigation; fetch the user once per session instead.
let currentCache: Promise<CurrentUser> | null = null;

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
