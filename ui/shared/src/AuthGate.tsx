import * as React from 'react';
import { DataProvider } from './DataProvider';

/* AuthGate — the login boundary for a production console.
 *
 * In `http` mode (the default in production) the app is gated behind a login: the
 * operator exchanges credentials for a PQ-signed token, and only then is the
 * authenticated `HttpGateway` injected and the console rendered against live
 * enclosure data. In `mock` mode (explicit offline demos only) it passes straight
 * through with the in-process mock — no auth. [RC-37] */

const TOKEN_KEY = 'midgard.session.token';
const SESSION_KEY = 'midgard.session.who';

/** Who signed in, as the login reported it (display only; the token decides). */
export interface Session {
  subject?: string;
  clearance?: string;
  roles?: string[];
}

export interface AuthGateProps<C> {
  /** `http` = authenticate against the edge; `mock` = offline demo, no auth. */
  mode: 'http' | 'mock';
  /** The edge API base, e.g. `http://127.0.0.1:7710/api/v1`. */
  baseUrl: string;
  /** The client used in `mock` mode. */
  mockClient: C;
  /** Build the authenticated client from a token and its signed-in subject (http
   *  mode, post-login). */
  makeClient: (token: string, session: Session) => C;
  /** Exchange credentials for a token (the SDK `login`). */
  login: (baseUrl: string, creds: { username: string; password: string }) => Promise<{ token: string; subject: string; clearance?: string; roles?: string[] }>;
  children: React.ReactNode;
}

export function AuthGate<C>(props: AuthGateProps<C>): React.ReactElement {
  const { mode, baseUrl, mockClient, makeClient, login, children } = props;

  const [token, setToken] = React.useState<string | null>(() => {
    if (mode !== 'http') return null;
    try {
      return sessionStorage.getItem(TOKEN_KEY);
    } catch {
      return null;
    }
  });
  const [session, setSession] = React.useState<Session>(() => {
    try {
      return JSON.parse(sessionStorage.getItem(SESSION_KEY) ?? '{}') as Session;
    } catch {
      return {};
    }
  });
  const [username, setUsername] = React.useState('');
  const [password, setPassword] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const client = React.useMemo<C>(
    () => (mode === 'mock' || !token ? mockClient : makeClient(token, session)),
    [mode, token, session, mockClient, makeClient],
  );

  // Mock mode, or already authenticated → render the app with its gateway.
  if (mode === 'mock' || token) {
    return <DataProvider client={client}>{children}</DataProvider>;
  }

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await login(baseUrl, { username, password });
      try {
        sessionStorage.setItem(TOKEN_KEY, res.token);
      } catch {
        /* session storage unavailable (private mode) — keep the token in memory */
      }
      const next: Session = { subject: res.subject, clearance: res.clearance, roles: res.roles };
      try {
        sessionStorage.setItem(SESSION_KEY, JSON.stringify(next));
      } catch {
        /* as above */
      }
      setSession(next);
      setToken(res.token);
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
