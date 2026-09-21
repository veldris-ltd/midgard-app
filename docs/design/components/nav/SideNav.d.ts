import * as React from 'react';

export interface SideNavItem { id: string; label: string; icon?: React.ReactNode; badge?: React.ReactNode; }
export interface SideNavSection { title?: string; items: SideNavItem[]; }

/** Console left rail on sunken ink; accent tint + 2px rail on the active item. */
export interface SideNavProps {
  sections?: SideNavSection[];
  activeId?: string;
  onSelect?: (id: string) => void;
  /** Pinned to the bottom */
  footer?: React.ReactNode;
  width?: number | string;
  style?: React.CSSProperties;
}
export declare function SideNav(props: SideNavProps): React.ReactElement;
