import * as React from 'react';

/** ProvenanceTag, the SAD provenance chip: [DD-4], [NFR-PQ-2], [STD:NIST FIPS 203]. */
export interface ProvenanceTagProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** e.g. "DD-4", "NFR-PQ-2", "STD:NIST FIPS 203" (no brackets) */
  id: string;
  /** Override inferred colour family */
  kind?: 'dd' | 'nfr' | 'fr' | 'std' | 'ac' | 'other';
  /** Tooltip text (the requirement statement) */
  title?: string;
  style?: React.CSSProperties;
}

export function ProvenanceTag({ id, kind, title, style, ...rest }: ProvenanceTagProps) {
  const k = kind || (String(id).startsWith('DD') ? 'dd' : String(id).startsWith('NFR') ? 'nfr' : String(id).startsWith('FR') ? 'fr' : String(id).startsWith('STD') ? 'std' : String(id).startsWith('AC') ? 'ac' : 'other');
  const colors: Record<string, string> = {
    dd:  'var(--accent-hover)',
    nfr: 'var(--info)',
    fr:  'var(--grey-300)',
    std: 'var(--secure)',
    ac:  'var(--warn)',
    other: 'var(--grey-400)',
  };
  return (
    <span
      title={title}
      style={{
        display: 'inline-flex', alignItems: 'center',
        padding: '1px 6px',
        fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)',
        letterSpacing: '0.05em',
        color: colors[k] || colors.other,
        background: 'var(--surface-inset)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius-xs)',
        whiteSpace: 'nowrap',
        cursor: title ? 'help' : 'default',
        ...style,
      }}
      {...rest}
    >[{id}]</span>
  );
}
