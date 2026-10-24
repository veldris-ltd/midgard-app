import * as React from 'react';
import { DataProvider } from './DataProvider';

/* AuthGate — the login boundary for a production console.
 *
 * In `http` mode (the default in production) the app is gated behind a login: the
 * operator exchanges credentials for a PQ-signed token, and only then is the
 * authenticated `HttpGateway` injected and the console rendered against live
 * enclosure data. In `mock` mode (explicit offline demos only) it passes straight
 * through with the in-process mock — no auth. A principal enrolled with a TOTP second
 * factor adds the code from their authenticator app; it is sent only when entered.
 * [RC-37] [RC-29] */

const TOKEN_KEY = 'midgard.session.token';

export interface AuthGateProps<C> {
  /** `http` = authenticate against the edge; `mock` = offline demo, no auth. */
  mode: 'http' | 'mock';
  /** The edge API base, e.g. `http://127.0.0.1:7710/api/v1`. */
  baseUrl: string;
  /** The client used in `mock` mode. */
  mockClient: C;
  /** Build the authenticated client from a token (http mode, post-login). */
  makeClient: (token: string) => C;
  /** Exchange credentials for a token (the SDK `login`). */
  login: (
    baseUrl: string,
    creds: { username: string; password: string; totp?: string },
  ) => Promise<{ token: string; subject: string }>;
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
  const [username, setUsername] = React.useState('');
  const [password, setPassword] = React.useState('');
  const [totp, setTotp] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const client = React.useMemo<C>(
    () => (mode === 'mock' || !token ? mockClient : makeClient(token)),
    [mode, token, mockClient, makeClient],
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
      const res = await login(baseUrl, totp ? { username, password, totp } : { username, password });
      try {
        sessionStorage.setItem(TOKEN_KEY, res.token);
      } catch {
        /* session storage unavailable (private mode) — keep the token in memory */
      }
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
