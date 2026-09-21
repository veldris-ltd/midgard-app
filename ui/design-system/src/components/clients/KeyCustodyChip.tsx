import * as React from 'react';

/** Device key-custody chip (FR-C-2): names the platform keystore and its minimum-security floor. Green = hardware-backed; amber = software floor; blue = brokered (extension holds no keys). */
export interface KeyCustodyChipProps extends React.HTMLAttributes<HTMLSpanElement> {
  platform?: 'ios' | 'macos' | 'android' | 'windows' | 'linux' | 'web' | 'extension';
  style?: React.CSSProperties;
}

type Platform = NonNullable<KeyCustodyChipProps['platform']>;

/** KeyCustodyChip — where this device's keys live and their security floor. */
export function KeyCustodyChip({ platform = 'ios', style, ...rest }: KeyCustodyChipProps) {
  const map: Record<Platform, { store: string; floor: string; c: string }> = {
    ios:      { store: 'SECURE ENCLAVE', floor: 'hardware · non-exportable', c: 'var(--secure)' },
    macos:    { store: 'SECURE ENCLAVE', floor: 'hardware · non-exportable', c: 'var(--secure)' },
    android:  { store: 'STRONGBOX', floor: 'hardware-backed', c: 'var(--secure)' },
    windows:  { store: 'TPM · DPAPI', floor: 'TPM-sealed', c: 'var(--secure)' },
    linux:    { store: 'SECRET SERVICE', floor: 'encrypted keyring · TPM optional', c: 'var(--warn)' },
    web:      { store: 'WEBCRYPTO', floor: 'non-extractable · session-scoped', c: 'var(--warn)' },
    extension:{ store: 'NO KEYS HELD', floor: 'brokered only', c: 'var(--info)' },
  };
  const k = map[platform] || map.ios;
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 7,
      padding: '3px 9px',
      fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.07em',
      background: 'var(--surface-inset)', border: '1px solid var(--border)',
      borderRadius: 'var(--radius-xs)', whiteSpace: 'nowrap',
      ...style,
    }} {...rest}>
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke={k.c} strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" style={{ flex: 'none' }}>
        <rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>
      </svg>
      <span style={{ color: k.c, fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'] }}>{k.store}</span>
      <span style={{ color: 'var(--text-faint)' }}>{k.floor}</span>
    </span>
  );
}
