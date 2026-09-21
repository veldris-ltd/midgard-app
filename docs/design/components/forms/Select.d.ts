import * as React from 'react';

/** Native select styled as a sunken ink well. */
export interface SelectProps {
  label?: string;
  /** Strings or {value,label} pairs */
  options?: Array<string | { value: string; label: string }>;
  value?: string;
  onChange?: (e: React.ChangeEvent<HTMLSelectElement>) => void;
  size?: 'sm' | 'md' | 'lg';
  fullWidth?: boolean;
  disabled?: boolean;
  style?: React.CSSProperties;
}
export declare function Select(props: SelectProps): React.ReactElement;
