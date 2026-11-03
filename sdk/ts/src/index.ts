/* @midgard/sdk — the Midgard TypeScript SDK.
 *
 * A typed domain model plus a single transport seam (`GatewayClient`). The UI
 * depends only on the interface; `HttpGateway` backs it against the enclosure edge.
 * The in-memory mocks and their fixtures are not here: they are the separate entry
 * `@midgard/sdk/mock`, which a console loads by dynamic import in mock mode only, so
 * an http build never bundles them. [VLD-ANB-MIDGARD-0086] [AC-25.2] */

export * from './types';
export type { GatewayClient } from './client';
export { HumanGateRequiredError, GatewayError } from './errors';
export { createHttpGateway } from './http/HttpGateway';
export type { HttpGatewayOptions } from './http/HttpGateway';
export { login, logout, whoami, sessionPolicy } from './http/login';
export { assertSafeBaseUrl } from './http/baseUrl';
export type { LoginCredentials, LoginResult, Session } from './http/login';

// Per-product gateway slices (one interface and its domain types each; the mocks are
// in @midgard/sdk/mock). All symbols are product-prefixed to avoid collisions across
// the shared barrel.
export * from './products/forge';
export * from './products/mimir';
export * from './products/bifrost';
export * from './products/heimdall';
export * from './products/admin';
export * from './products/clients';
