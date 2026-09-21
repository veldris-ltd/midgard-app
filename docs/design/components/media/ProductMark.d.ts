import * as React from 'react';

/** Typographic product identity: initial tile + wordmark. Plain type by design; no logos were provided or invented. */
export interface ProductMarkProps {
  product?: 'midgard' | 'forge' | 'mimir' | 'bifrost' | 'heimdall' | 'admin';
  /** Show the mono descriptor line under the wordmark */
  showDescriptor?: boolean;
  size?: 'sm' | 'md' | 'lg';
  style?: React.CSSProperties;
}
export declare function ProductMark(props: ProductMarkProps): React.ReactElement;
