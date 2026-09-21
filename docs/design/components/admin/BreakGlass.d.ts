import * as React from 'react';

/** Break-glass emergency-elevation strip (FR-AD-8). idle = sealed chip; active = pulsing red with countdown; review = amber post-hoc review demand. */
export interface BreakGlassProps {
  state?: 'idle' | 'active' | 'review';
  holder?: string;
  /** What the elevation covers — never more than the emergency requires */
  scope?: string;
  /** Countdown label while active */
  remaining?: string;
  justification?: string;
  style?: React.CSSProperties;
}
export declare function BreakGlass(props: BreakGlassProps): React.ReactElement;
