# Porting note — docs/design → @midgard/design-system

The **authority** for the design system remains `docs/design/` (per `ui/README.md`).
This package is its bundled port for app toolchains. Keep the two in sync when either changes.

## What was ported, and how
| Source (docs/design) | Here | Change on port |
|---|---|---|
| `tokens/*.css` + `styles.css` | `src/tokens/*.css` | Copied **verbatim**. `styles.css` is the CSS entry (`@midgard/design-system/tokens.css`). |
| `docs/mode.js` | `src/mode.ts` | Rewritten as an ES module (`getMode/setMode/resolveMode/onModeChange/initMode`); same `localStorage('mg-mode')`, `.mode-light`, and `mg-mode` event. |
| `components/**/**.jsx` (28) | `src/components/**/**.tsx` (28) | `.jsx`→`.tsx`: added `import * as React`, typed props from each sibling `.d.ts`, kept every inline style + `var(--…)` token byte-for-byte. |
| (implicit runtime scope) | explicit imports | The prototypes shared one global scope (ds-loader strips import/export). Ported components add explicit cross-component imports where needed. |
| — | `src/ThemeScope.tsx` | New helper applying `.theme-<product>` (one accent per screen). |

## Conventions preserved
- Inline-style, token-driven authoring; hover/press/toggle via `useState` (no `:hover`).
- Classification markings keep their **fixed** colours (`--marking-*`) regardless of theme/mode.
- `Icon` carries the same ~90 named Lucide-style paths + `ICON_NAMES`.
- Never emoji; ALL-CAPS mono labels; `ProductMark` is typographic (no invented logos).

## Not ported (out of scope this pass)
The marketing `site/` chrome (`site-shell.jsx`, `fabric-bg.js`) and the client device frames
(`ui_kits/clients/*.jsx`). `ModeToggle` was ported into `@midgard/ui-shared` (it belongs with the shell).
