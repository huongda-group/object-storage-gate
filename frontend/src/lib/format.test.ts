import { describe, expect, it } from "vitest";
import { fmt, grp, pill, quotaView, shortId } from "./format";

describe("fmt", () => {
  it("returns 0 B for zero", () => expect(fmt(0)).toBe("0 B"));
  it("keeps bytes integral", () => expect(fmt(512)).toBe("512 B"));
  it("drops a trailing .0", () => expect(fmt(1024)).toBe("1 KiB"));
  it("uses one decimal above KiB", () => expect(fmt(1536)).toBe("1.5 KiB"));
  it("stops at TiB", () => expect(fmt(1024 ** 5)).toBe("1024 TiB"));
});

describe("grp", () => {
  it("groups thousands with dots", () => expect(grp(128431)).toBe("128.431"));
});

describe("quotaView", () => {
  it("treats max=0 as unlimited", () => {
    const q = quotaView(12 * 1024 ** 3, 0);
    expect(q.unlimited).toBe(true);
    expect(q.maxText).toBe("Không giới hạn");
    expect(q.barW).toBe("0%");
    expect(q.state).toBe("Không giới hạn");
  });

  it("computes percent, colour and remaining", () => {
    const q = quotaView(50 * 1024 ** 3, 100 * 1024 ** 3);
    expect(q.barW).toBe("50.0%");
    expect(q.color).toBe("var(--ok)");
    expect(q.state).toBe("Còn 50 GiB");
    expect(q.pctText).toBe("50 GiB / 100 GiB (50.0%)");
    expect(q.usedLine).toBe("50 GiB / 100 GiB");
  });

  it("warns at 75% and above", () => {
    expect(quotaView(80, 100).color).toBe("var(--warn)");
  });

  it("flags nearly-full at 90% and above", () => {
    const q = quotaView(95, 100);
    expect(q.color).toBe("var(--dgr)");
    expect(q.state).toBe("Sắp đầy");
  });

  it("clamps used above max to 100%", () => {
    expect(quotaView(200, 100).barW).toBe("100.0%");
  });

  it("caps the reserved segment at the remaining width", () => {
    const q = quotaView(90, 100, 50);
    expect(q.barW).toBe("90.0%");
    expect(q.resW).toBe("10.0%");
  });
});

describe("pill", () => {
  it("maps every status", () => {
    expect(pill("active").pill).toBe("Đang hoạt động");
    expect(pill("disabled").pill).toBe("Tạm khoá");
    expect(pill("expired").pill).toBe("Hết hạn");
    expect(pill("revoked").pill).toBe("Đã thu hồi");
  });
});

describe("shortId", () => {
  it("keeps 7 leading and 4 trailing chars", () => {
    expect(shortId("OSG3f7a91d0c4b29b2c")).toBe("OSG3f7a…9b2c");
  });
});
