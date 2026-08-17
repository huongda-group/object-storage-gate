import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ApiError,
  api,
  changePassword,
  clearToken,
  getToken,
  setToken,
} from "./auth";

afterEach(() => {
  localStorage.clear();
  sessionStorage.clear();
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

  it("keeps an unremembered token out of localStorage", () => {
    setToken("abc", false);
    expect(localStorage.getItem("osg_token")).toBeNull();
    expect(sessionStorage.getItem("osg_token")).toBe("abc");
    expect(getToken()).toBe("abc");
  });

  it("does not leave a stale copy behind when the mode changes", () => {
    setToken("abc", true);
    setToken("def", false);
    expect(localStorage.getItem("osg_token")).toBeNull();
    expect(getToken()).toBe("def");
  });
});

describe("api", () => {
  it("omits Authorization when there is no token", async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response("{}", { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await api("/api/auth/current");
    const headers = new Headers(fetchMock.mock.calls[0][1]?.headers);
    expect(headers.has("Authorization")).toBe(false);
  });

  it("attaches the bearer token when present", async () => {
    setToken("abc");
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response("{}", { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await api("/api/auth/current");
    const headers = new Headers(fetchMock.mock.calls[0][1]?.headers);
    expect(headers.get("Authorization")).toBe("Bearer abc");
  });

  it("sets JSON content type for bodies", async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response("{}", { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await api("/api/auth/login", { method: "POST", body: "{}" });
    const headers = new Headers(fetchMock.mock.calls[0][1]?.headers);
    expect(headers.get("Content-Type")).toBe("application/json");
  });

  it("throws ApiError carrying the status", async () => {
    vi.stubGlobal("fetch", async () => new Response("nope", { status: 401 }));
    await expect(api("/api/auth/current")).rejects.toMatchObject({
      status: 401,
    });
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

describe("changePassword", () => {
  it("posts current and new password with the bearer token", async () => {
    setToken("tok-123");
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response("", { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await changePassword("old-one", "new-one-please");

    const [path, init] = fetchMock.mock.calls[0];
    expect(path).toBe("/api/me/password");
    expect(init?.method).toBe("POST");
    expect(JSON.parse(init?.body as string)).toEqual({
      current_password: "old-one",
      new_password: "new-one-please",
    });
    expect(new Headers(init?.headers).get("Authorization")).toBe(
      "Bearer tok-123",
    );
  });
});
