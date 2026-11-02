import * as React from 'react';
import { DataProvider } from './DataProvider';
import { signOutAtServer, type Logout } from './signOut';

/* AuthGate — the login boundary for a production console.
 *
 * In `http` mode (the default in production) the app is gated behind a login: the
 * operator exchanges credentials for a PQ-signed token, and only then is the
 * authenticated `HttpGateway` injected and the console rendered against live
 * enclosure data. In `mock` mode (explicit offline demos only) it passes straight
 * through with the in-process mock — no auth. A principal enrolled with a TOTP second
 * factor adds the code from their authenticator app; it is sent only when entered.
 *
 * The token is held in React state only, never in web storage, so no script on the
 * origin can read it back from there. A reload therefore returns to sign-in, by
 * design. Signing out tells the edge first (`logout`), which revokes the token so a
 * copy of it is refused too, and only then clears the session here.
 * [RC-37] [RC-29] [VLD-ANB-MIDGARD-0021] */
 *
 * The mock client is optional so that an http build need not construct, or bundle, a
 * mock: a console in mock mode loads one from `@midgard/sdk/mock` and passes it here.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2] */

const EXPIRED = 'Your session has ended. Sign in again.';

/** Who is signed in, as the edge reads it from the token. */
export interface SessionIdentity {
  subject: string;
  roles: string[];
  clearance: string;
  compartments: string[];
  purpose: string;
  /** Token expiry (unix seconds). */
  expiresAt: number;
}

export interface AuthGateProps<C, S extends SessionIdentity = SessionIdentity> {
  /** `http` = authenticate against the edge; `mock` = offline demo, no auth. */
  mode: 'http' | 'mock';
  /** The edge API base, e.g. `http://127.0.0.1:7710/api/v1`. */
  baseUrl: string;
  /** The client used in `mock` mode; required there, and never needed in `http` mode. */
  mockClient?: C;
  /** Build the authenticated client for a session (http mode, post-login). It must
   *  call `onUnauthorized` when the edge answers 401, which ends the session. */
  makeClient: (token: string, session: S, onUnauthorized: () => void) => C;
  /** Exchange credentials for a token (the SDK `login`). */
  login: (
    baseUrl: string,
    creds: { username: string; password: string; totp?: string },
  ) => Promise<{ token: string; subject: string }>;
  /** Who a token belongs to (the SDK `whoami`); rejects with `status` 401 for a token
   *  that has expired or no longer verifies. */
  whoami: (baseUrl: string, token: string) => Promise<S>;
  /** End a session at the edge (the SDK `logout`), so the token is revoked there and
   *  not merely forgotten here. Without it, signing out only clears this console. */
  logout?: Logout;
  children: React.ReactNode;
}

interface SessionState {
  /** The signed-in identity; `null` in mock mode. */
  session: SessionIdentity | null;
  /** End the session at the edge, then return to sign-in; `null` in mock mode. */
  signOut: (() => void) | null;
}

const SessionContext = React.createContext<SessionState>({ session: null, signOut: null });

/** The signed-in session and a way to end it, for the console frame (the Account
 *  menu) and any screen that shows who is acting. [RC-37] */
export function useSession(): SessionState {
  return React.useContext(SessionContext);
}

export function AuthGate<C, S extends SessionIdentity = SessionIdentity>(
  props: AuthGateProps<C, S>,
): React.ReactElement {
  const { mode, baseUrl, mockClient, makeClient, login, whoami, logout, children } = props;
  if (mode === 'mock' && mockClient === undefined) {
    throw new Error(
      "AuthGate: mode 'mock' needs a mockClient; load one with await import('@midgard/sdk/mock')",
    );
  }

  // Memory only: a reload starts at sign-in.
  const [token, setToken] = React.useState<string | null>(null);
  const [session, setSession] = React.useState<S | null>(null);
  const [username, setUsername] = React.useState('');
  const [password, setPassword] = React.useState('');
  const [totp, setTotp] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);

  const end = React.useCallback((message: string | null) => {
    setToken(null);
    setSession(null);
    setNotice(message);
  }, []);
  const expire = React.useCallback(() => end(EXPIRED), [end]);
  const signOut = React.useCallback(() => {
    void signOutAtServer(logout, baseUrl, token, end);
  }, [logout, baseUrl, token, end]);

  // The token's own expiry ends the session on time, not at the next failed request.
  React.useEffect(() => {
    if (!session) return undefined;
    const ms = session.expiresAt * 1000 - Date.now();
    if (ms <= 0) {
      expire();
      return undefined;
    }
    const t = setTimeout(expire, Math.min(ms, 2 ** 31 - 1));
    return () => clearTimeout(t);
  }, [session, expire]);

  const client = React.useMemo<C | undefined>(
    () => (mode === 'mock' || !token || !session ? mockClient : makeClient(token, session, expire)),
    [mode, token, session, mockClient, makeClient, expire],
  );
  const state = React.useMemo<SessionState>(
    () => (mode === 'mock' ? { session: null, signOut: null } : { session, signOut }),
    [mode, session, signOut],
  );

  if (mode === 'mock' || (token && session)) {
    return (
      <SessionContext.Provider value={state}>
        <DataProvider client={client}>{children}</DataProvider>
      </SessionContext.Provider>
    );
  }
  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const res = await login(baseUrl, totp ? { username, password, totp } : { username, password });
      const s = await whoami(baseUrl, res.token);
      setSession(s);
      setToken(res.token);
      setPassword('');
      setTotp('');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Login failed');
    } finally {
      setBusy(false);
    }
  };

  // Minimal in-enclosure login card (design-system-agnostic inline styles).
  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'grid',
        placeItems: 'center',
        background: 'var(--bg-base, #0a0a0a)',
        color: 'var(--text-body, #e6e6e6)',
        fontFamily: 'var(--font-sans, system-ui, sans-serif)',
      }}
    >
      <form
        onSubmit={onSubmit}
        style={{
          width: 340,
          padding: 28,
          border: '1px solid var(--border, #262626)',
          borderRadius: 10,
          background: 'var(--bg-raised, #121212)',
          display: 'flex',
          flexDirection: 'column',
          gap: 14,
        }}
      >
        <div style={{ fontSize: 18, fontWeight: 700, letterSpacing: '0.02em' }}>Midgard — sign in</div>
        <div style={{ fontSize: 12, color: 'var(--text-muted, #9a9a9a)' }}>
          Authenticate to the enclosure edge. Your session token is PQ-signed (ML-DSA).
        </div>
        <label style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 12 }}>
          Username
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoComplete="username"
            style={inputStyle}
          />
        </label>
        <label style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 12 }}>
          Password
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
            style={inputStyle}
          />
        </label>
        <label style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 12 }}>
          Authenticator code
          <input
            value={totp}
            onChange={(e) => setTotp(e.target.value.replace(/\D/g, '').slice(0, 6))}
            inputMode="numeric"
            autoComplete="one-time-code"
            placeholder="Only if you use an authenticator app"
            aria-describedby="totp-hint"
            style={inputStyle}
          />
          <span id="totp-hint" style={{ color: 'var(--text-muted, #9a9a9a)' }}>
            The 6-digit code from your authenticator app, if your account has one.
          </span>
        </label>
        {notice && !error && (
          <div role="status" style={{ fontSize: 12, color: 'var(--text-muted, #9a9a9a)' }}>
            {notice}
          </div>
        )}
        {error && (
          <div role="alert" style={{ fontSize: 12, color: 'var(--accent-danger, #e5484d)' }}>
            {error}
          </div>
        )}
        <button
          type="submit"
          disabled={busy || !username || !password}
          style={{
            marginTop: 4,
            padding: '10px 12px',
            borderRadius: 8,
            border: 'none',
            fontWeight: 600,
            cursor: busy ? 'progress' : 'pointer',
            background: 'var(--accent, #ca1d25)',
            color: '#fff',
            opacity: busy || !username || !password ? 0.6 : 1,
          }}
        >
          {busy ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  padding: '9px 10px',
  borderRadius: 8,
  border: '1px solid var(--border, #262626)',
  background: 'var(--bg-sunken, #0d0d0d)',
  color: 'inherit',
  fontSize: 14,
};
