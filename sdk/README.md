# sdk/ — generated, typed SDKs

Typed TypeScript and Python SDKs (plus a CLI and MCP endpoint) giving developers
and agents governed access to objects, actions, and functions. `[DD-6]`
SAD_00 §6.2.8.

- `ts/` — TypeScript SDK (generated into `src/gen`, gitignored)
- `py/` — Python SDK (generated into `src/gen`, gitignored)

Both are generated from `contracts/proto` via `just gen`. Do not hand-edit
generated code; change the proto and regenerate.
