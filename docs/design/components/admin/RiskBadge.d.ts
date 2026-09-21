import * as React from 'react';

/** Admin action risk level 1–5 (KDD-2). Risk ≥4 implies dual-control unless overridden. */
export interface RiskBadgeProps {
  level?: 1 | 2 | 3 | 4 | 5;
  /** Force the DUAL-CONTROL flag on/off; null = inferred from level */
  dualControl?: boolean | null;
  style?: React.CSSProperties;
}
export declare function RiskBadge(props: RiskBadgeProps): React.ReactElement;
