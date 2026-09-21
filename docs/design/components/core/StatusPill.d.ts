import * as React from 'react';

/** Live state pill with leading (optionally pulsing) dot. */
export interface StatusPillProps {
  status?: 'secure' | 'online' | 'healthy' | 'offline' | 'warn' | 'danger' | 'live' | 'gated';
  /** Override the default status text */
  label?: string;
  /** Pulse animation on positive/live states */
  pulse?: boolean;
  style?: React.CSSProperties;
}
export declare function StatusPill(props: StatusPillProps): React.ReactElement;
