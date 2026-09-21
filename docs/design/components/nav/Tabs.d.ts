import * as React from 'react';

export interface TabDef { label: string; icon?: React.ReactNode; badge?: React.ReactNode; }

/** Underline tabs; 2px accent rule marks the active tab. */
export interface TabsProps {
  tabs?: Array<string | TabDef>;
  active?: number;
  onChange?: (index: number) => void;
  size?: 'sm' | 'md';
  style?: React.CSSProperties;
}
export declare function Tabs(props: TabsProps): React.ReactElement;
