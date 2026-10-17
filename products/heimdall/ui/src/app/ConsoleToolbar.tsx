import * as React from 'react';
import { useNavigate } from 'react-router-dom';
import { Icon, IconButton, Input } from '@midgard/design-system';
import { ModeToggle } from '@midgard/ui-shared';
import { AlertsBell } from './alerts';
import { useStatus } from './status';

/* Standard top-bar cluster shared by every Heimdall screen: scoped search, the
 * colour-mode toggle, and alert/settings affordances. The bell shows the live open
 * alert count and opens the alerts on the operating picture. [FR-H-1] */
export function ConsoleToolbar({ placeholder = 'Search entities, feeds, tracks…' }: { placeholder?: string }) {
  const navigate = useNavigate();
  const status = useStatus();
  return (
    <>
      <Input
        mono
        size="sm"
        placeholder={placeholder}
        prefix={<Icon name="search" size={14} color="var(--text-faint)" />}
        style={{ width: 'min(280px, 42vw)' }}
      />
      <ModeToggle />
      <AlertsBell onClick={() => { status.setAlertsOpen(true); navigate('/'); }} />
      <IconButton label="Settings">
        <Icon name="settings" size={17} />
      </IconButton>
    </>
  );
}
