import { ApiError } from "./auth";

/** Turns a failure into something a person can act on. */
function messageFor(e: unknown): string {
  if (e instanceof ApiError) {
    if (e.status >= 500) return "Máy chủ gặp lỗi. Thử lại sau.";
    if (e.status === 403) return "Bạn không có quyền thực hiện thao tác này.";
    if (e.status === 404) return "Không tìm thấy. Có thể nó đã bị xoá.";
    if (e.status === 429) return "Thao tác quá nhanh. Chờ một chút rồi thử lại.";
    return e.message || "Yêu cầu không hợp lệ.";
  }
  if (e instanceof TypeError) return "Không kết nối được máy chủ.";
  return "Có lỗi không xác định.";
}

/**
 * Runs an API call and reports failures instead of dropping them.
 *
 * Every mutation in this console used to be `onClick={() => void doThing()}`, so a rejected
 * request produced no toast, no error and no state change — the user saw nothing at all and
 * clicked again.
 */
export async function run<T>(
  fn: () => Promise<T>,
  opts: { onError?: (message: string) => void } = {},
): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    opts.onError?.(messageFor(e));
    return undefined;
  }
}
