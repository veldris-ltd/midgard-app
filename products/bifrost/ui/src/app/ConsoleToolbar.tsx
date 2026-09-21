import * as React from 'react';
import { Icon, IconButton, Input } from '@midgard/design-system';
import { ModeToggle } from '@midgard/ui-shared';

/* Standard top-bar cluster shared by every Bifrost screen: scoped search, the
 * colour-mode toggle, and account/notification affordances. */
export function ConsoleToolbar({ placeholder = 'Search releases, artefacts, rings…' }: { placeholder?: string }) {
  return (
    <>
      <Input
        mono
        size="sm"
        placeholder={placeholder}
        prefix={<Icon name="search" size={14} color="var(--text-faint)" />}
        style={{ width: 'min(300px, 42vw)' }}
      />
      <ModeToggle />
      <IconButton label="Notifications">
        <Icon name="bell" size={17} />
      </IconButton>
      <IconButton label="Account">
        <Icon name="user" size={17} />
      </IconButton>
    </>
  );
}
