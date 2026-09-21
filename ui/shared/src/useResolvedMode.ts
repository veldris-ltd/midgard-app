import * as React from 'react';
import { onModeChange, resolveMode, type ResolvedMode } from '@midgard/design-system';

/* The resolved colour mode, kept current as the ModeToggle (or, in 'system'
 * mode, the OS preference) changes it. Tokens follow the mode on their own;
 * this is for presentational pieces that paint fixed platform hexes, such as
 * device frames, and so have to choose between two palettes themselves. */
export function useResolvedMode(): ResolvedMode {
  return React.useSyncExternalStore(onModeChange, () => resolveMode(), () => 'dark');
}
