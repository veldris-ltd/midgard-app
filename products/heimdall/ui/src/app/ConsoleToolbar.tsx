import * as React from 'react';
import { Icon, IconButton, Input } from '@midgard/design-system';
import { ModeToggle } from '@midgard/ui-shared';

/* Standard top-bar cluster shared by every Heimdall screen: scoped search, the
 * colour-mode toggle, and alert/settings affordances. */
export function ConsoleToolbar({ placeholder = 'Search entities, feeds, tracks…' }: { placeholder?: string }) {
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
      <IconButton label="Alerts">
        <Icon name="bell" size={17} />
      </IconButton>
      <IconButton label="Settings">
        <Icon name="settings" size={17} />
      </IconButton>
    </>
  );
}
