import * as React from 'react';

/** AI inference tier badge (FR-C-6): on-device (green), remote enclosure (blue), hybrid (accent), disabled (grey). Every AI answer on a client carries one. */
export interface TierBadgeProps {
  tier?: 'on-device' | 'remote' | 'hybrid' | 'disabled';
  /** Model name, e.g. "GEMMA INT4" */
  model?: string;
  /** Explicit data_leaves_device readout */
  leaves?: boolean | null;
  style?: React.CSSProperties;
}
export declare function TierBadge(props: TierBadgeProps): React.ReactElement;
