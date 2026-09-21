import * as React from 'react';

export interface TableColumn<Row = any> {
  header: React.ReactNode;
  /** Row property to read (or provide cell) */
  key?: keyof Row & string;
  /** Custom renderer */
  cell?: (row: Row, index: number) => React.ReactNode;
  /** Render cell in mono (ids, hashes, timestamps) */
  mono?: boolean;
  align?: 'left' | 'right' | 'center';
  width?: number | string;
}

/** Dense console data table: mono ALL-CAPS headers, 40px rows (32 compact), accent-rail selection. */
export interface TableProps<Row = any> extends Omit<React.HTMLAttributes<HTMLDivElement>, 'onClick'> {
  columns: TableColumn<Row>[];
  rows: Row[];
  /** 32px rows */
  compact?: boolean;
  selectedIndex?: number;
  onRowClick?: (index: number, row: Row) => void;
  style?: React.CSSProperties;
}

export function Table<Row = any>({ columns = [], rows = [], compact = false, selectedIndex = -1, onRowClick, style, ...rest }: TableProps<Row>) {
  const [hoverRow, setHoverRow] = React.useState(-1);
  const rowH = compact ? 'var(--row-h-compact)' : 'var(--row-h)';
  return (
    <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflow: 'hidden', background: 'var(--surface-card)', ...style }} {...rest}>
      <table style={{ width: '100%', borderCollapse: 'collapse' }}>
        <thead>
          <tr style={{ background: 'var(--surface-inset)' }}>
            {columns.map((c, i) => (
              <th key={i} style={{
                height: 32, padding: '0 14px', textAlign: c.align || 'left',
                fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)',
                fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'], letterSpacing: 'var(--ls-label)',
                textTransform: 'uppercase', color: 'var(--text-faint)',
                borderBottom: '1px solid var(--border)', whiteSpace: 'nowrap',
                width: c.width,
              }}>{c.header}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, ri) => {
            const selected = ri === selectedIndex;
            return (
              <tr
                key={ri}
                onClick={onRowClick ? () => onRowClick(ri, row) : undefined}
                onMouseEnter={() => setHoverRow(ri)}
                onMouseLeave={() => setHoverRow(-1)}
                style={{
                  background: selected ? 'var(--accent-tint)' : hoverRow === ri && onRowClick ? 'var(--surface-raised)' : 'transparent',
                  boxShadow: selected ? 'inset 2px 0 0 var(--accent)' : 'none',
                  cursor: onRowClick ? 'pointer' : 'default',
                  transition: 'background var(--dur-fast) var(--ease-out)',
                }}
              >
                {columns.map((c, ci) => (
                  <td key={ci} style={{
                    height: rowH, padding: '0 14px', textAlign: c.align || 'left',
                    fontFamily: c.mono ? 'var(--font-mono)' : 'var(--font-sans)',
                    fontSize: c.mono ? 'var(--fs-xs)' : 'var(--fs-sm)',
                    color: ci === 0 ? 'var(--grey-100)' : 'var(--text-body)',
                    borderBottom: ri < rows.length - 1 ? '1px solid var(--divider)' : 'none',
                    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 320,
                  }}>{typeof c.cell === 'function' ? c.cell(row, ri) : c.key != null ? (row[c.key] as React.ReactNode) : null}</td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
