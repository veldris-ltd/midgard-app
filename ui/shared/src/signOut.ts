/* Signing out of a console: the edge is told first, so it revokes the token and
 * refuses any copy of it, and only then is the session cleared here. Kept apart from
 * `AuthGate` so the order can be tested without a browser. [VLD-ANB-MIDGARD-0021] */

/** The notice the sign-in card shows after a sign-out the edge recorded. */
export const SIGNED_OUT = 'Signed out.';

/** Ends a session at the edge (the SDK `logout`). */
export type Logout = (baseUrl: string, token: string) => Promise<void>;

/**
 * Revoke `token` at the edge with `logout`, then `end` the local session. The local
 * session ends whatever happens: if the edge cannot be told, the notice says that
 * the token stays valid there until it expires, rather than claiming a sign-out the
 * edge never saw. Without a `logout` (or a token) only the local session ends.
 */
export async function signOutAtServer(
  logout: Logout | undefined,
  baseUrl: string,
  token: string | null,
  end: (message: string) => void,
): Promise<void> {
  if (logout && token) {
    try {
      await logout(baseUrl, token);
    } catch (e) {
      end(
        `Signed out of this console, but the gateway could not be told ` +
          `(${e instanceof Error ? e.message : String(e)}): the session stays valid there until it expires.`,
      );
      return;
    }
  }
  end(SIGNED_OUT);
}
