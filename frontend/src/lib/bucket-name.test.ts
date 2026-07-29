import { describe, expect, it } from "vitest";
import { validateBucketName } from "./bucket-name";

const existing = ["media-cdn", "backup-db"];

describe("validateBucketName", () => {
  it("says nothing about an empty field", () => {
    expect(validateBucketName("", existing)).toBe("");
  });

  it("accepts a valid name", () => {
    expect(validateBucketName("logs-nginx", existing)).toBe("");
    expect(validateBucketName("a1.b2-c3", existing)).toBe("");
  });

  it("rejects too short and too long", () => {
    expect(validateBucketName("ab", existing)).toMatch("3–63");
    expect(validateBucketName("a".repeat(64), existing)).toMatch("3–63");
  });

  it("rejects uppercase, underscores and edge punctuation", () => {
    expect(validateBucketName("Media", existing)).toMatch("chữ thường");
    expect(validateBucketName("media_cdn", existing)).toMatch("chữ thường");
    expect(validateBucketName("-media", existing)).toMatch("chữ thường");
    expect(validateBucketName("media-", existing)).toMatch("chữ thường");
  });

  it("rejects consecutive dots", () => {
    expect(validateBucketName("me..dia", existing)).toMatch("hai dấu chấm");
  });

  it("rejects a name the account already owns", () => {
    expect(validateBucketName("media-cdn", existing)).toMatch("409");
  });
});
