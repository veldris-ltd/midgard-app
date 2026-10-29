import * as React from 'react';
import { createRoot } from 'react-dom/client';
import { initMode, setMode, type Mode } from '@midgard/design-system';
import { App } from './App';

import '@midgard/design-system/tokens.css';
import './play.css';

// Apply the persisted colour mode (shared with the consoles) and follow the OS.
initMode();

// A change made inside a console frame reaches the shell as a storage event:
// apply it here too, so the shell and every console stay in one mode. [ADR-PLAY-002]
window.addEventListener('storage', (e) => {
  if (e.key === 'mg-mode' && (e.newValue === 'dark' || e.newValue === 'light' || e.newValue === 'system')) {
    setMode(e.newValue as Mode);
  }
});

createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
