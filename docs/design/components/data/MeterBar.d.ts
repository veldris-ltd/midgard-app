import * as React from 'react';

/** Labelled meter for utilisation, pass rates, confidence, rollout progress. Threshold props recolour: green when safe, warn/danger past thresholds (invert for lower-is-worse metrics). */
export interface MeterBarProps {
  /** 0–100 */
  value?: number;
  label?: string;
  showValue?: boolean;
  /** Turns warn at/above this value (or at/below when invert) */
  warnAt?: number | null;
  dangerAt?: number | null;
  /** Lower values are worse (pass rates, confidence) */
  invert?: boolean;
  width?: number;
  format?: (v: number) => React.ReactNode;
  style?: React.CSSProperties;
}
export declare function MeterBar(props: MeterBarProps): React.ReactElement;
