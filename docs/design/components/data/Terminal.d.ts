import * as React from 'react';

/** Monospace command/output block. Lines starting "> " get an accent prompt caret; "#" lines render faint. */
export interface TerminalProps {
  /** Optional title bar text (mono caps) */
  title?: string;
  /** Lines of text or nodes */
  lines?: Array<string | React.ReactNode>;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}
export declare function Terminal(props: TerminalProps): React.ReactElement;
