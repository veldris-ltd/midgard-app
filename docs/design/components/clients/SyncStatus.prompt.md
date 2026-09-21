SyncStatus is the client sync chip (FR-C-4): offline shows queued CRDT ops, syncing spins while draining, merged confirms a conflict-free reconnect.

```jsx
<SyncStatus state="offline" queued={7} />
<SyncStatus state="merged" />
```

Offline is a first-class state, never an error tone.