/* @midgard/design-system — public surface.
 *
 * The Midgard design system, ported from the docs/design prototypes into a
 * bundled TypeScript package. Tokens ship as CSS (`@midgard/design-system/tokens.css`);
 * components are token-driven React and re-theme via a `ThemeScope` (one accent
 * per screen) and the light/dark/system mode manager. */

// Theme + mode ----------------------------------------------------------------
export { ThemeScope } from './ThemeScope';
export type { ThemeScopeProps, ProductId } from './ThemeScope';
export { getMode, setMode, resolveMode, onModeChange, initMode } from './mode';
export type { Mode, ResolvedMode } from './mode';

// core ------------------------------------------------------------------------
export * from './components/core/Button';
export * from './components/core/IconButton';
export * from './components/core/Badge';
export * from './components/core/Tag';
export * from './components/core/StatusPill';

// forms -----------------------------------------------------------------------
export * from './components/forms/Input';
export * from './components/forms/Select';
export * from './components/forms/Switch';

// data ------------------------------------------------------------------------
export * from './components/data/Card';
export * from './components/data/Table';
export * from './components/data/StatBlock';
export * from './components/data/SectionLabel';
export * from './components/data/Terminal';
export * from './components/data/ProvenanceTag';
export * from './components/data/MeterBar';

// nav -------------------------------------------------------------------------
export * from './components/nav/Tabs';
export * from './components/nav/SideNav';

// governance ------------------------------------------------------------------
export * from './components/governance/ClassificationBanner';
export * from './components/governance/HumanGate';
export * from './components/governance/MarkingChip';

// admin -----------------------------------------------------------------------
export * from './components/admin/RiskBadge';
export * from './components/admin/GuardedAction';
export * from './components/admin/BreakGlass';

// clients ---------------------------------------------------------------------
export * from './components/clients/SyncStatus';
export * from './components/clients/TierBadge';
export * from './components/clients/KeyCustodyChip';

// media -----------------------------------------------------------------------
export * from './components/media/Icon';
export * from './components/media/ProductMark';
