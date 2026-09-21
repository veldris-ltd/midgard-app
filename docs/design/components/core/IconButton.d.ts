import * as React from 'react';

/** Square icon-only action button. */
export interface IconButtonProps {
  /** sm 28 · md 36 · lg 44 px square */
  size?: 'sm' | 'md' | 'lg';
  /** 'ghost' = bare; 'solid' = ink surface + border */
  variant?: 'ghost' | 'solid';
  /** Accent-tinted active state */
  active?: boolean;
  disabled?: boolean;
  /** Accessible label (also the tooltip) */
  label?: string;
  onClick?: (e: React.MouseEvent) => void;
  /** Usually an <Icon /> */
  children?: React.ReactNode;
  style?: React.CSSProperties;
}
export declare function IconButton(props: IconButtonProps): React.ReactElement;
