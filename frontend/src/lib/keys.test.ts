import { describe, expect, it } from "vitest";
import { expiryLabel } from "./keys";

describe("expiryLabel", () => {
  it("returns null when the key never expires", () => {
    expect(expiryLabel(null)).toBeNull();
  });

  it("counts remaining days", () => {
    expect(expiryLabel(3)).toBe("Còn 3 ngày");
  });

  it("says expired on the day it lapses", () => {
    expect(expiryLabel(0)).toBe("Hết hạn hôm nay");
  });
});
