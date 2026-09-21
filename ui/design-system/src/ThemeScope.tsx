import * as React from 'react';

/* One accent per screen. `ThemeScope` applies the product's `.theme-<product>`
   class so every descendant reads that product's `--accent-*` tokens. Default
   scope is Midgard red (matches the tokens' :root). */

export type ProductId = 'midgard' | 'forge' | 'mimir' | 'bifrost' | 'heimdall';

export interface ThemeScopeProps {
  product: ProductId;
  children?: React.ReactNode;
  style?: React.CSSProperties;
  className?: string;
  as?: keyof JSX.IntrinsicElements;
}

export function ThemeScope({ product, children, style, className, as = 'div' }: ThemeScopeProps) {
  const Tag = as as React.ElementType;
  const cls = `theme-${product}${className ? ' ' + className : ''}`;
  return (
    <Tag className={cls} style={style}>
      {children}
    </Tag>
  );
}
