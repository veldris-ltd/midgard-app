import * as React from 'react';
import { useNavigate } from 'react-router-dom';
import { Icon, IconButton, Input } from '@midgard/design-system';
import { AccountMenu, ModeToggle } from '@midgard/ui-shared';

/* Standard top-bar cluster shared by every Midgard screen: search (Enter opens Data
 * search for that text across every object type), the colour-mode toggle,
 * notifications, and the Account menu with who is signed in and Sign out.
 * [RC-37] [DD-1] */
export function ConsoleToolbar({ placeholder = 'Search all object data…' }: { placeholder?: string }) {
  const navigate = useNavigate();
  const [text, setText] = React.useState('');
  return (
    <>
      <Input
        mono
        size="sm"
        placeholder={placeholder}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && text.trim()) navigate(`/data?q=${encodeURIComponent(text.trim())}`);
        }}
        title="Press Enter to search every object type for this text"
        aria-label="Search every object type"
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
