import * as React from 'react';

export interface TableColumn {
  header: string;
  /** Row property to read (or provide cell) */
  key?: string;
  /** Custom renderer */
  cell?: (row: any, index: number) => React.ReactNode;
  /** Render cell in mono (ids, hashes, timestamps) */
  mono?: boolean;
  align?: 'left' | 'right' | 'center';
  width?: number | string;
}

/** Dense console data table: mono ALL-CAPS headers, 40px rows (32 compact), accent-rail selection. */
export interface TableProps {
  columns?: TableColumn[];
  rows?: any[];
  /** 32px rows */
  compact?: boolean;
  selectedIndex?: number;
  onRowClick?: (index: number) => void;
  style?: React.CSSProperties;
}
export declare function Table(props: TableProps): React.ReactElement;
