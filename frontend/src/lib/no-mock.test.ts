import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) return walk(full);
    return full.endsWith(".tsx") || full.endsWith(".ts") ? [full] : [];
  });
}

/**
 * Nine screens once rendered fixture data as if it were the signed-in user's account, and the
 * fixtures shipped in the production bundle. This is the cheapest guard against that coming
 * back: no source file may import the mock module.
 */
describe("no fixture data in the app", () => {
  it("no source file imports lib/mock", () => {
    const offenders = walk("src")
      .filter((f) => !f.endsWith("no-mock.test.ts"))
      .filter((f) =>
        /from\s+["'][^"']*lib\/mock["']|from\s+["']\.\.?\/mock["']/.test(
          readFileSync(f, "utf8"),
        ),
      );

    expect(offenders).toEqual([]);
  });
});
