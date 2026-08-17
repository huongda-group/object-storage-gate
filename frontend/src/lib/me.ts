import { type CurrentUser, api } from "./auth";

/**
 * Renames the calling user.
 *
 * Deliberately narrow: role and quota are an admin's decision, and the server ignores anything
 * else sent here.
 */
export const updateMe = (name: string) =>
  api<CurrentUser>("/api/me", {
    method: "PATCH",
    body: JSON.stringify({ name }),
  });
