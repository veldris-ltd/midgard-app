import * as React from 'react';

/** Full-width classification strip (top of screen). Fixed colours, theme-independent. */
export interface ClassificationBannerProps {
  level?: 'confidential' | 'restricted' | 'internal' | 'open';
  /** Override the default banner text */
  text?: string;
  /** 20px instead of 26px */
  compact?: boolean;
  style?: React.CSSProperties;
}
export declare function ClassificationBanner(props: ClassificationBannerProps): React.ReactElement;
