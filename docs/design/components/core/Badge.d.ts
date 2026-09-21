import * as React from 'react';

/** Small mono count / flag chip. */
export interface BadgeProps {
  tone?: 'neutral' | 'accent' | 'secure' | 'warn' | 'danger' | 'info';
  children?: React.ReactNode;
  style?: React.CSSProperties;
}
export declare function Badge(props: BadgeProps): React.ReactElement;
