import * as React from 'react';

/** Terminal, monospace command/output block on a sunken well. */
export interface TerminalProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Optional title bar text (mono caps) */
  title?: string;
  /** Lines of text or nodes */
  lines?: Array<string | React.ReactNode>;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}

export function Terminal({ title, lines = [], children, style, ...rest }: TerminalProps) {
  return (
    <div style={{
      background: 'var(--bg-sunken)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--radius-md)',
      overflow: 'hidden',
      ...style,
    }} {...rest}>
      {title && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '8px 14px', borderBottom: '1px solid var(--border)',
          background: 'var(--surface-inset)',
        }}>
          <span style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--accent)', opacity: 0.9 }} />
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: 'var(--ls-label)', textTransform: 'uppercase', color: 'var(--text-faint)' }}>{title}</span>
        </div>
      )}
      <div style={{ padding: '14px 16px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', lineHeight: 1.9, color: 'var(--grey-300)', overflowX: 'auto' }}>
        {lines.map((l, i) => {
          if (typeof l === 'string') {
            const isPrompt = l.startsWith('> ');
            const isComment = l.startsWith('#');
            return (
              <div key={i} style={{ whiteSpace: 'pre', color: isComment ? 'var(--text-faint)' : 'inherit' }}>
                {isPrompt ? <><span style={{ color: 'var(--accent)' }}>&gt;</span>{l.slice(1)}</> : l}
              </div>
            );
          }
          return <div key={i} style={{ whiteSpace: 'pre' }}>{l}</div>;
        })}
        {children}
      </div>
    </div>
  );
}
