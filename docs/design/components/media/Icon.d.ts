import * as React from 'react';

/** Single-stroke line glyph (~1.9px), Lucide-substituted. Includes platform (lock, shield-check, key, boxes, network, database, server, terminal, fingerprint), Forge (hammer, git-branch, workflow, table), Mimir (brain-circuit, bot, sparkles, gauge, cpu), Bifrost (rocket, package, radio-tower, undo-2, rings), Heimdall (eye, radar, map, crosshair, layers) and generic UI names. */
export interface IconProps {
  /** Lucide-style name, e.g. "shield-check", "git-branch", "radar" */
  name: string;
  size?: number;
  strokeWidth?: number;
  color?: string;
  style?: React.CSSProperties;
}
export declare function Icon(props: IconProps): React.ReactElement;
export declare const ICON_NAMES: string[];
