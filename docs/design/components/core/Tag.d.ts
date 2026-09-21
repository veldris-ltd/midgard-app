import * as React from 'react';

/** Removable / selectable label chip (filters, topics). */
export interface TagProps {
  /** Show a × remove affordance */
  onRemove?: () => void;
  /** Accent-selected state */
  active?: boolean;
  onClick?: () => void;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}
export declare function Tag(props: TagProps): React.ReactElement;
