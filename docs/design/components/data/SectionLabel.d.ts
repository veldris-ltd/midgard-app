import * as React from 'react';

/** Numbered mono eyebrow: 01 // IDENTITY. */
export interface SectionLabelProps {
  /** Section number; zero-padded automatically */
  number?: number | string;
  children?: React.ReactNode;
  /** Grey instead of accent numeral */
  muted?: boolean;
  style?: React.CSSProperties;
}
export declare function SectionLabel(props: SectionLabelProps): React.ReactElement;
