import * as React from 'react';
import { Icon, IconButton, Input } from '@midgard/design-system';
import { AccountMenu, ModeToggle } from '@midgard/ui-shared';

/* Standard top-bar cluster shared by every Midgard screen: scoped search, the
 * colour-mode toggle, notifications, and the Account menu with who is signed in and
 * Sign out. [RC-37] */
export function ConsoleToolbar({ placeholder = 'Search objects, links, actions…' }: { placeholder?: string }) {
  return (
    <>
      <Input
        mono
        size="sm"
        placeholder={placeholder}
        disabled
        title="Not available yet: search is coming with the data search page"
        prefix={<Icon name="search" size={14} color="var(--text-faint)" />}
        style={{ width: 'min(300px, 42vw)' }}
      />
      <ModeToggle />
      <IconButton label="Notifications (not available yet)" disabled>
        <Icon name="bell" size={17} />
      </IconButton>
      <AccountMenu />
    </>
  );
}
