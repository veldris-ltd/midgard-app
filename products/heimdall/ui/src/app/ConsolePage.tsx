import * as React from 'react';
import { useNavigate } from 'react-router-dom';
import { ConsoleShell, type ConsoleShellProps } from '@midgard/ui-shared';
import { liveBadges, liveSeals, useStatus } from './status';

/* Router-aware wrapper around the shared ConsoleShell: injects `onNavigate` so
 * sidenav selection drives react-router instead of a full page load. Every
 * Heimdall screen renders through this, passing its own `active`, `context`,
 * `toolbar`, and inspector `aside`. The header seals and nav badges are the live
 * values from the status provider, not the NAV defaults; wall mode hides the nav.
 * [FR-H-1] [FR-P3-7] */
export type ConsolePageProps = Omit<ConsoleShellProps, 'product' | 'onNavigate'>;

export function ConsolePage(props: ConsolePageProps) {
  const navigate = useNavigate();
  const status = useStatus();
  const onNavigate = React.useCallback((href: string) => navigate(href), [navigate]);
  return (
    <ConsoleShell
      product="heimdall"
      onNavigate={onNavigate}
      seals={liveSeals(status)}
      badges={liveBadges(status)}
      hideNav={status.wall}
      {...props}
    />
  );
}
