# products/ — the four products (Phases 4–7)

Each is independently useful atop Midgard core and composes with the others
`[DD-6]`. Each has a backend and a UI built under its design-system theme scope
(exactly one accent per screen).

| Directory | Product | Backend | Theme | Phase / Source |
|---|---|---|---|---|
| `forge/` | Forge — data operations | Rust (DataFusion) | ember | 4 / `SAD_01` |
| `bifrost/` | Bifrost — continuous delivery | Go | violet | 5 / `SAD_03` |
| `heimdall/` | Heimdall — operational fusion | Rust | gold | 6 / `SAD_04` |
| `admin/` | Admin Control Panel — composes subsystem APIs | Go (thin) | red | 7 / `SAD_07` |

Each product dir holds `services/` (or `engine/`/`controller/`) and `ui/`
(React + TS, consuming `ui/design-system`). The admin panel **composes** existing
subsystem admin APIs and is bound by the same policy/audit — no privileged bypass.

> **Status: scaffold only.** Built per phase. Acceptance: AC-F-*, AC-B-*, AC-H-*, AC-AD-*.
