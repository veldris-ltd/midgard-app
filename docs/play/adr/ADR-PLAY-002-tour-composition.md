# ADR-PLAY-002: One origin, consoles in a frame, a guided tour driven by data

| | |
|---|---|
| Status | Accepted |
| Date | 29 October 2026 |
| Owner | Veldris Ltd |
| Related | ADR-PLAY-001, ADR-PLAY-003 |

## Context

A visitor needs two things: free exploration of any console, and a guided path that explains what they are looking at, in a sensible order, without having to know the product. The consoles must stay unmodified (ADR-PLAY-001), so the tour cannot live inside them.

## Decision

1. **Layout.** The shell has two views:
   - **Launcher** (`#/`): what the playground is, a card per console, and a start button for the tour.
   - **Workspace** (`#/tour/<step>` and `#/console/<product>`): the console in an `<iframe>` taking most of the screen, beside a panel. In tour mode the panel shows the current step; in free mode it shows the console's sections and a link back into the tour.
2. **Same origin.** Consoles are served from the same origin as the shell (`/consoles/<product>/`). The shell can therefore read the frame's document to find and highlight elements, and the consoles share the `mg-mode` colour choice through `localStorage` (`ui/design-system/src/mode.ts:16,43`), so one toggle themes everything.
3. **Steps are data.** Every step lives in `play/src/tour.ts` as a typed record: product, route, title, body, an optional "try this" instruction, and an optional highlight target given as visible text (for example `Ground aircraft`). Changing the tour is a data edit with no layout code.
4. **Highlighting** finds the smallest element in the frame whose visible text matches the target, scrolls it into view and draws a ring over it from the shell's own overlay. The ring follows scrolling and resizing. A target can also be an icon-only control's accessible name (`aria-label` or `title`). On narrow screens the consoles fold their detail rail behind an "Expand inspector" control; when a step's target is inside it, the shell rings that control and tells the visitor to open it, then moves the ring to the target once it appears. The shell never presses it. If neither is found, the step still works and nothing is drawn.
5. **Deep links.** `#/tour/7` opens step 7 and `#/console/heimdall` opens Heimdall in free mode, so a link in a talk, an email or a README can point at a specific moment.
6. **Keyboard and assistive technology.** Left and right arrow keys move through the tour when focus is in the panel; the panel is a labelled region, the step counter is announced politely, and every control is a real button or link. The frame has a title naming the console.

## Alternatives considered

- **Tour hooks inside each console** (data attributes, a tour SDK). Rejected: it changes seven consoles for a showcase and couples them to it.
- **Highlighting by CSS selector.** Rejected as the default: class names in the consoles are not a stable contract. Visible text is what the visitor is told to look for anyway. If a target ever becomes ambiguous, a selector field can be added to the step record.
- **A third-party tour library.** Rejected: it adds a dependency and its own styling for a few hundred lines of behaviour; the design system already provides the look.
- **Separate origins per console.** Rejected: cross-origin frames cannot be highlighted or themed together.

## Consequences

- A console change that renames the visible text a step points at removes that step's highlight but not the step. `play/scripts/check.mjs` reports every step whose route or target is missing, so the drift is caught at build verification, not by visitors.
- The shell needs no knowledge of a console beyond its name, routes and the strings in `tour.ts`.
