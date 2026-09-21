import * as React from 'react';

/** Device key-custody chip (FR-C-2): names the platform keystore and its minimum-security floor. Green = hardware-backed; amber = software floor; blue = brokered (extension holds no keys). */
export interface KeyCustodyChipProps {
  platform?: 'ios' | 'macos' | 'android' | 'windows' | 'linux' | 'web' | 'extension';
  style?: React.CSSProperties;
}
export declare function KeyCustodyChip(props: KeyCustodyChipProps): React.ReactElement;
