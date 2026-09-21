import * as React from 'react';

/** Flat ink panel. Elevation = lighter surface + border, no shadow at rest. */
export interface CardProps {
  /** Lifts one ink step on hover */
  interactive?: boolean;
  /** 2px accent top rule */
  accentRule?: boolean;
  /** Padding: sm 14 · md 20 · lg 28 (lg also uses radius-lg) */
  size?: 'sm' | 'md' | 'lg';
  onClick?: () => void;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}
export declare function Card(props: CardProps): React.ReactElement;
