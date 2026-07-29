/** S3 bucket naming plus "unique per user" — Buckets.dc.html validateBucket(). */
export function validateBucketName(
  name: string,
  existingNames: string[],
): string {
  if (!name) return "";
  if (name.length < 3 || name.length > 63) return "Tên phải dài 3–63 ký tự.";
  if (!/^[a-z0-9][a-z0-9.-]*[a-z0-9]$/.test(name))
    return 'Chỉ chữ thường, số, dấu "-" và "."; phải bắt đầu và kết thúc bằng chữ hoặc số.';
  if (name.includes("..")) return "Không được có hai dấu chấm liền nhau.";
  if (existingNames.includes(name))
    return `Bucket "${name}" đã tồn tại trong tài khoản của bạn (409).`;
  return "";
}
