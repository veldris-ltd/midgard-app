import * as React from 'react';
import { Button, Icon, IconButton } from '@midgard/design-system';
import { useSession } from './AuthGate';

/* AccountMenu — the top bar's Account control: who is signed in, what they hold, and
 * Sign out. In mock mode there is no session, so it says so instead. [RC-37] */
export function AccountMenu() {
  const { session, signOut } = useSession();
  const [open, setOpen] = React.useState(false);
  const root = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!open) return undefined;
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const until = session ? new Date(session.expiresAt * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '';

  return (
    <div ref={root} style={{ position: 'relative' }}>
      <IconButton label="Account" active={open} aria-expanded={open} aria-haspopup="dialog" onClick={() => setOpen((o) => !o)}>
        <Icon name="user" size={17} />
      </IconButton>
      {open && (
        <div
          role="dialog"
          aria-label="Account"
          style={{
            position: 'absolute', right: 0, top: 'calc(100% + 6px)', zIndex: 200, width: 260,
            padding: 14, display: 'flex', flexDirection: 'column', gap: 10,
            background: 'var(--surface-raised, var(--bg-raised))', border: '1px solid var(--border)',
            borderRadius: 'var(--radius-md)', boxShadow: 'var(--shadow-lg)',
          }}
        >
          {session ? (
            <>
              <div>
                <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-heading)', fontWeight: 600, overflowWrap: 'anywhere' }}>{session.subject}</div>
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.06em', color: 'var(--text-faint)', marginTop: 4 }}>
                  {(session.roles.join(' · ') || 'NO ROLES').toUpperCase()} · {session.clearance.toUpperCase()}
                  {session.compartments.length > 0 && ` // ${session.compartments.join('/')}`}
                </div>
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.06em', color: 'var(--text-faint)', marginTop: 2 }}>
                  SESSION ENDS {until}
                </div>
              </div>
              <Button variant="secondary" size="sm" fullWidth iconLeft={<Icon name="lock" size={14} />} onClick={() => signOut?.()}>
                Sign out
              </Button>
            </>
          ) : (
            <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>Offline demo: no one is signed in.</div>
          )}
        </div>
      )}
    </div>
  );
}
