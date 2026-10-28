/* @midgard/ui-shared — the shared console frame + data seam wiring.
 *
 * Built on @midgard/design-system (components/tokens) and @midgard/sdk (the
 * GatewayClient). Product consoles compose `ConsoleShell` + screens and read
 * data through the `DataProvider` hooks. */

export { ConsoleShell } from './ConsoleShell';
export type { ConsoleShellProps } from './ConsoleShell';
export { NAV } from './nav';
export type { NavConfig, NavItem, NavSection, Seal, SealKind, ConsoleProduct } from './nav';
export { useMedia, Scrim, PanelHandle } from './responsive';
export type { PanelHandleProps } from './responsive';
export { DataProvider, useGateway, useAsync, useAuditStream } from './DataProvider';
export type { AsyncState } from './DataProvider';
export { AuthGate, useSession } from './AuthGate';
export type { AuthGateProps, SessionIdentity } from './AuthGate';
export { AccountMenu } from './AccountMenu';
export { ModeToggle } from './ModeToggle';
export { useResolvedMode } from './useResolvedMode';
