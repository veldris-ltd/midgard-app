import * as React from 'react';

/** Text field on a sunken ink well; accent focus ring. */
export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  /** ALL-CAPS mono label above the field */
  label?: string;
  hint?: string;
  /** Error message; also turns the border red */
  error?: string;
  /** Monospace input text (ids, keys, commands) */
  mono?: boolean;
  size?: 'sm' | 'md' | 'lg';
  prefix?: React.ReactNode;
  suffix?: React.ReactNode;
  fullWidth?: boolean;
  style?: React.CSSProperties;
  inputStyle?: React.CSSProperties;
}
export declare function Input(props: InputProps): React.ReactElement;
