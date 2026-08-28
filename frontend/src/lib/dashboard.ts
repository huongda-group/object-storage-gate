import type { Summary } from "./auth";
import { getSummary } from "./auth";
import { type Bucket, listBuckets } from "./buckets";
import { type ApiKey, listKeys } from "./keys";

/**
 * The gateway's own origin.
 *
 * The screens used to print a hardcoded `https://s3.osgate.vn` into the copy-paste connection
 * snippets, so a user copying `aws s3 cp … --endpoint-url …` pointed their credentials at a
 * domain that was not their deployment.
 */
export function endpoint(): string {
  return globalThis.location?.origin ?? "";
}

export type DashboardData = {
  summary: Summary;
  buckets: Bucket[];
  keys: ApiKey[];
};

export async function loadDashboard(): Promise<DashboardData> {
  const [summary, buckets, keys] = await Promise.all([
    getSummary(),
    listBuckets(),
    listKeys(),
  ]);
  return { summary, buckets, keys };
}

/** Byte units the quota editor offers, largest last. */
export const UNITS: Record<string, number> = {
  MiB: 1024 ** 2,
  GiB: 1024 ** 3,
  TiB: 1024 ** 4,
};
