import * as React from 'react';

/** Big Oxanium numeral over a mono ALL-CAPS label. */
export interface StatBlockProps {
  /** The number/readout, e.g. "99.9%" */
  value: React.ReactNode;
  /** Mono label, e.g. "CONTROL-PLANE SLO" */
  label: string;
  /** Optional small sub-line */
  sub?: React.ReactNode;
  size?: 'sm' | 'md' | 'lg';
  /** Colour the numeral in the product accent */
  accent?: boolean;
  style?: React.CSSProperties;
}
export declare function StatBlock(props: StatBlockProps): React.ReactElement;
