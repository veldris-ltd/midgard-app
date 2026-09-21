import * as React from 'react';

/** Offline-first sync chip (FR-C-4): offline (with queued op count), syncing (spinner), synced, merged. */
export interface SyncStatusProps {
  state?: 'offline' | 'syncing' | 'synced' | 'merged';
  /** Locally queued CRDT ops while offline/draining */
  queued?: number;
  /** Override the default detail text */
  detail?: string;
  style?: React.CSSProperties;
}
export declare function SyncStatus(props: SyncStatusProps): React.ReactElement;
