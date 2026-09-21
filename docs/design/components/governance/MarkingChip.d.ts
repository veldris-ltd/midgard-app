import * as React from 'react';

/** Per-object classification marking chip (role/marking/purpose access control). */
export interface MarkingChipProps {
  level?: 'confidential' | 'restricted' | 'internal' | 'open';
  /** Override the level text */
  label?: string;
  /** Compartment suffix, e.g. "OPS-N" */
  compartment?: string;
  style?: React.CSSProperties;
}
export declare function MarkingChip(props: MarkingChipProps): React.ReactElement;
