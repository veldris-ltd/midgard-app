import * as React from 'react';

/**
 * Primary action primitive. Crisp corners, product-accent fill, glow on hover.
 * @startingPoint section="Components" subtitle="Action button, primary / secondary / outline / ghost" viewport="700x260"
 */
export interface ButtonProps {
  /** Visual weight. 'primary' = accent fill (one per view), 'secondary' = ink surface, 'outline' = accent border, 'ghost' = bare. */
  variant?: 'primary' | 'secondary' | 'outline' | 'ghost';
  /** sm 30px · md 38px · lg 48px */
  size?: 'sm' | 'md' | 'lg';
  fullWidth?: boolean;
  disabled?: boolean;
  iconLeft?: React.ReactNode;
  iconRight?: React.ReactNode;
  type?: 'button' | 'submit' | 'reset';
  onClick?: (e: React.MouseEvent) => void;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}
export declare function Button(props: ButtonProps): React.ReactElement;
