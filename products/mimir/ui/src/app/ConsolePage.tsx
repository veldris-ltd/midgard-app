import * as React from 'react';
import { useNavigate } from 'react-router-dom';
import { ConsoleShell, type ConsoleShellProps } from '@midgard/ui-shared';

/* Router-aware wrapper around the shared ConsoleShell: injects `onNavigate` so
 * sidenav selection drives react-router instead of a full page load. Every
 * Mimir screen renders through this, passing its own `active`, `context`,
 * `toolbar`, and inspector `aside`. */
export type ConsolePageProps = Omit<ConsoleShellProps, 'product' | 'onNavigate'>;

export function ConsolePage(props: ConsolePageProps) {
  const navigate = useNavigate();
  const onNavigate = React.useCallback((href: string) => navigate(href), [navigate]);
  return <ConsoleShell product="mimir" onNavigate={onNavigate} {...props} />;
}
