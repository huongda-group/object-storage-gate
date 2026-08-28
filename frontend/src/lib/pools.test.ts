import { describe, expect, it, vi } from "vitest";
import { setToken } from "./auth";
import { createPool, updatePool } from "./pools";

function stubFetch() {
  const fetchMock = vi.fn(
    async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response("{}", { status: 200 }),
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

describe("pools api", () => {
  it("posts a pool with its credentials", async () => {
    setToken("tok");
    const fetchMock = stubFetch();

    await createPool({
      name: "main",
      provider: "minio",
      region: "ap-southeast-1",
      api_endpoint: "https://minio.internal:9000",
      physical_bucket: "osg-main",
      access_id: "ID",
      access_secret: "SECRET",
    });

    const [path, init] = fetchMock.mock.calls[0];
    expect(path).toBe("/api/admin/pools");
    expect(JSON.parse(init?.body as string).access_secret).toBe("SECRET");
  });

  it("omits access_secret when the field was left blank", async () => {
    setToken("tok");
    const fetchMock = stubFetch();

    await updatePool("some-pid", { region: "us-east-1", access_secret: "" });

    const [, init] = fetchMock.mock.calls[0];
    const body = JSON.parse(init?.body as string);
    expect(body.region).toBe("us-east-1");
    expect("access_secret" in body).toBe(false);
  });

  it("keeps a blank string out of every field, not just the secret", async () => {
    setToken("tok");
    const fetchMock = stubFetch();

    await updatePool("some-pid", {
      region: "",
      api_endpoint: "https://s3.example.com",
      physical_bucket: "",
    });

    const [, init] = fetchMock.mock.calls[0];
    const body = JSON.parse(init?.body as string);
    expect(body).toEqual({ api_endpoint: "https://s3.example.com" });
  });
});
