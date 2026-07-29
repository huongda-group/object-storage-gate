// Fixtures copied from the Claude Design prototypes in console-object-storage-gate/project/.
// Replaced call-site by call-site once slice #7 exposes the real API.
import type { KeyStatus } from "./format";

const K = 1024;
const M = 1024 ** 2;
const G = 1024 ** 3;
const T = 1024 ** 4;

export const ENDPOINT = "https://s3.osgate.vn";
export const REGION = "ap-southeast-1";

export type Bucket = {
  name: string;
  used: number;
  max: number;
  res: number;
  objects: number;
  created: string;
  full: string;
};

// Buckets.dc.html BUCKETS()
export const BUCKETS: Bucket[] = [
  {
    name: "media-cdn",
    used: 42.7 * G,
    max: 50 * G,
    res: 0,
    objects: 128431,
    created: "3 ngày trước",
    full: "25/07/2026 09:12 (+07)",
  },
  {
    name: "backup-db",
    used: 188.2 * G,
    max: 200 * G,
    res: 0,
    objects: 2140,
    created: "14/03/2026",
    full: "14/03/2026 02:00 (+07)",
  },
  {
    name: "logs-nginx",
    used: 12.4 * G,
    max: 0,
    res: 0,
    objects: 984220,
    created: "02/01/2026",
    full: "02/01/2026 11:40 (+07)",
  },
  {
    name: "static-assets",
    used: 3.2 * G,
    max: 10 * G,
    res: 512 * M,
    objects: 5127,
    created: "6 ngày trước",
    full: "22/07/2026 16:05 (+07)",
  },
  {
    name: "sandbox-test",
    used: 0,
    max: 1 * G,
    res: 0,
    objects: 0,
    created: "hôm qua",
    full: "27/07/2026 10:22 (+07)",
  },
];

// Account totals — Dashboard.dc.html renderVals()
export const ACCOUNT = { used: 246.5 * G, max: 300 * G, res: 2.1 * G };
export const ACCOUNT_STATS = { buckets: "5", objects: "1.12M", keys: "3" };

export type S3Object = {
  key: string;
  size: number;
  contentType: string;
  etag: string;
  updated: string;
};

// Bucket Detail.dc.html OBJECTS()
export const OBJECTS: S3Object[] = [
  {
    key: "photos/2026/07/hero-01.png",
    size: 2.4 * M,
    contentType: "image/png",
    etag: "a91f3c07",
    updated: "2 giờ trước",
  },
  {
    key: "photos/2026/07/hero-02.png",
    size: 3.1 * M,
    contentType: "image/png",
    etag: "5c02de88",
    updated: "2 giờ trước",
  },
  {
    key: "photos/2026/06/banner-sale.jpg",
    size: 890 * K,
    contentType: "image/jpeg",
    etag: "77b1aa20",
    updated: "12/06/2026 08:31",
  },
  {
    key: "photos/2025/12/tet-campaign.jpg",
    size: 1.6 * M,
    contentType: "image/jpeg",
    etag: "de440193",
    updated: "22/12/2025 19:04",
  },
  {
    key: "videos/promo-q3-1080p.mp4",
    size: 412.8 * M,
    contentType: "video/mp4",
    etag: "0b9ce671",
    updated: "4 ngày trước",
  },
  {
    key: "videos/promo-q3-720p.mp4",
    size: 188.2 * M,
    contentType: "video/mp4",
    etag: "ff3a12c9",
    updated: "4 ngày trước",
  },
  {
    key: "thumbs/hero-01@2x.webp",
    size: 84 * K,
    contentType: "image/webp",
    etag: "3311bd05",
    updated: "2 giờ trước",
  },
  {
    key: "thumbs/hero-02@2x.webp",
    size: 91 * K,
    contentType: "image/webp",
    etag: "9a70e4f2",
    updated: "2 giờ trước",
  },
  {
    key: "index.json",
    size: 12.4 * K,
    contentType: "application/json",
    etag: "cc10ab73",
    updated: "hôm nay 09:12",
  },
  {
    key: "manifest.json",
    size: 4.2 * K,
    contentType: "application/json",
    etag: "6e2299fa",
    updated: "hôm nay 09:12",
  },
  {
    key: "robots.txt",
    size: 128,
    contentType: "text/plain",
    etag: "10ee44c0",
    updated: "02/01/2026 11:40",
  },
];

export type Permission =
  | "read"
  | "write"
  | "delete"
  | "list"
  | "multipart"
  | "presigned";

export type AccessKey = {
  id: string;
  label: string;
  status: KeyStatus;
  created: string;
  exp: string | null;
  expSoon?: boolean;
  perms: Permission[];
  prefixes: string[];
};

// Access Keys.dc.html KEYS(), created dates from Dashboard.dc.html KEYS()
export const KEYS: AccessKey[] = [
  {
    id: "OSG3f7a91d0c4b29b2c",
    label: "primary",
    status: "active",
    created: "12/01/2026",
    exp: null,
    perms: ["read", "write", "list", "multipart"],
    prefixes: [],
  },
  {
    id: "OSGb21c77af3e5144de",
    label: "ci",
    status: "active",
    created: "20/07/2026",
    exp: "Còn 3 ngày",
    expSoon: true,
    perms: ["read", "list"],
    prefixes: ["builds/*"],
  },
  {
    id: "OSG9d0244be71a3c7f1",
    label: "backup",
    status: "disabled",
    created: "08/11/2025",
    exp: null,
    perms: ["read", "write", "delete", "list", "multipart", "presigned"],
    prefixes: ["backup/*", "snapshots/*"],
  },
  {
    id: "OSG5ae8c1930b6f0a13",
    label: "temporary",
    status: "expired",
    created: "02/04/2026",
    exp: "Hết hạn 02/07",
    perms: ["read", "write", "list"],
    prefixes: ["tmp/*"],
  },
  {
    id: "OSG7c4419d0aa2fbe90",
    label: "readonly",
    status: "revoked",
    created: "15/05/2026",
    exp: null,
    perms: ["read", "list"],
    prefixes: [],
  },
];

// The one-time secret the prototypes show after create / rotate.
export const NEW_KEY = {
  id: "OSG8d21f4a0c7be5591",
  secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzEXAMPLEKEY",
};

export type AdminUser = {
  email: string;
  name: string;
  role: "user" | "admin";
  used: number;
  max: number;
  buckets: number;
  keys: string;
  ver: boolean;
  created: string;
};

// Admin Users.dc.html USERS() + created from Admin User Detail.dc.html USERS()
export const ADMIN_USERS: AdminUser[] = [
  {
    email: "an.nguyen@osgate.vn",
    name: "An Nguyễn",
    role: "admin",
    used: 246.5 * G,
    max: 300 * G,
    buckets: 5,
    keys: "4/5",
    ver: true,
    created: "02/09/2025",
  },
  {
    email: "team-ops@osgate.vn",
    name: "Đội Ops",
    role: "user",
    used: 480 * G,
    max: 500 * G,
    buckets: 12,
    keys: "9/11",
    ver: true,
    created: "11/10/2025",
  },
  {
    email: "data@osgate.vn",
    name: "Data Platform",
    role: "user",
    used: 1.2 * T,
    max: 0,
    buckets: 7,
    keys: "3/3",
    ver: true,
    created: "04/02/2026",
  },
  {
    email: "odoo-backup@osgate.vn",
    name: "Sao lưu Odoo",
    role: "user",
    used: 193 * G,
    max: 200 * G,
    buckets: 2,
    keys: "1/1",
    ver: true,
    created: "21/12/2025",
  },
  {
    email: "cdn@osgate.vn",
    name: "Hạ tầng CDN",
    role: "user",
    used: 142 * G,
    max: 200 * G,
    buckets: 4,
    keys: "3/4",
    ver: true,
    created: "30/03/2026",
  },
  {
    email: "minh.tran@osgate.vn",
    name: "Minh Trần",
    role: "user",
    used: 88 * G,
    max: 100 * G,
    buckets: 3,
    keys: "2/2",
    ver: true,
    created: "17/05/2026",
  },
  {
    email: "qa@osgate.vn",
    name: "QA Automation",
    role: "user",
    used: 6.4 * G,
    max: 50 * G,
    buckets: 2,
    keys: "2/3",
    ver: false,
    created: "09/06/2026",
  },
  {
    email: "intern@osgate.vn",
    name: "Thực tập sinh",
    role: "user",
    used: 0,
    max: 20 * G,
    buckets: 0,
    keys: "0/1",
    ver: false,
    created: "hôm qua",
  },
];

// Admin.dc.html adminStats — pre-formatted, kept verbatim
export const ADMIN_STATS = [
  {
    label: "USER",
    value: "8",
    sub: "1 admin · 2 chưa xác thực",
    color: "var(--tx)",
  },
  { label: "BUCKET", value: "35", sub: "trên 8 tài khoản", color: "var(--tx)" },
  { label: "OBJECT", value: "2.41M", sub: "metadata rows", color: "var(--tx)" },
  {
    label: "DUNG LƯỢNG DÙNG",
    value: "2.3 TiB",
    sub: "trên 4 TiB vật lý",
    color: "var(--tx)",
  },
  {
    label: "QUOTA ĐÃ CẤP",
    value: "5.1 TiB",
    sub: "oversubscribe 127%",
    color: "var(--warn)",
  },
];

export const GRANTED_QUOTA_LINE = "5.1 TiB quota đã cấp";

export type PoolBucket = {
  name: string;
  owner: string | null;
  used: number;
  max: number;
  objects: number;
  created: string;
  full: string;
  provider?: string;
  region?: string;
  apiEndpoint?: string;
  accessId?: string;
  accessSecret?: string;
  publicEnabled?: boolean;
};

// Admin Buckets.dc.html BUCKETS()
export const POOL_BUCKETS: PoolBucket[] = [
  {
    name: "system-archive",
    owner: null,
    used: 890 * G,
    max: 0,
    objects: 41200,
    created: "11/02/2026",
    full: "11/02/2026 08:00 (+07)",
  },
  {
    name: "gateway-audit-logs",
    owner: null,
    used: 64.3 * G,
    max: 500 * G,
    objects: 2140310,
    created: "02/01/2026",
    full: "02/01/2026 00:00 (+07)",
  },
  {
    name: "media-cdn",
    owner: "cdn@osgate.vn",
    used: 42.7 * G,
    max: 50 * G,
    objects: 128431,
    created: "25/07/2026",
    full: "25/07/2026 09:12 (+07)",
    provider: "r2",
    region: "apac",
    apiEndpoint: "https://<account-id>.r2.cloudflarestorage.com",
    accessId: "R2AK7X9Q2M4N",
    accessSecret: "••••••••••••••••••••",
    publicEnabled: true,
  },
  {
    name: "backup-db",
    owner: "odoo-backup@osgate.vn",
    used: 188.2 * G,
    max: 200 * G,
    objects: 2140,
    created: "14/03/2026",
    full: "14/03/2026 02:00 (+07)",
    provider: "aws",
    region: "ap-southeast-1",
    apiEndpoint: "https://s3.ap-southeast-1.amazonaws.com",
    accessId: "AKIA3F8QZ1M0X9K2",
    accessSecret: "••••••••••••••••••••",
    publicEnabled: false,
  },
  {
    name: "logs-nginx",
    owner: "team-ops@osgate.vn",
    used: 12.4 * G,
    max: 0,
    objects: 984220,
    created: "02/01/2026",
    full: "02/01/2026 11:40 (+07)",
  },
  {
    name: "static-assets",
    owner: "an.nguyen@osgate.vn",
    used: 3.2 * G,
    max: 10 * G,
    objects: 5127,
    created: "22/07/2026",
    full: "22/07/2026 16:05 (+07)",
  },
  {
    name: "sandbox-test",
    owner: "minh.tran@osgate.vn",
    used: 0,
    max: 1 * G,
    objects: 0,
    created: "hôm qua",
    full: "27/07/2026 10:22 (+07)",
  },
  {
    name: "warehouse-raw",
    owner: "data@osgate.vn",
    used: 1.2 * T,
    max: 0,
    objects: 318402,
    created: "18/06/2026",
    full: "18/06/2026 14:30 (+07)",
  },
];

// Admin Buckets.dc.html PROVIDERS()
export const PROVIDERS = [
  { value: "internal", label: "Nội bộ (Gateway mặc định)" },
  { value: "aws", label: "AWS S3" },
  { value: "r2", label: "Cloudflare R2" },
  { value: "b2", label: "Backblaze B2" },
  { value: "spaces", label: "DigitalOcean Spaces" },
  { value: "minio", label: "MinIO tự host" },
  { value: "custom", label: "Khác (S3-compatible tuỳ chỉnh)" },
];

export const UNITS: Record<string, number> = { MiB: M, GiB: G, TiB: T };
