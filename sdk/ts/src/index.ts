/* @midgard/sdk — the Midgard TypeScript SDK.
 *
 * A typed domain model plus a single transport seam (`GatewayClient`). The UI
 * depends only on the interface; `MockGateway` backs it today, `HttpGateway`
 * (a stub) is where a real browser-reachable bridge wires in. */

export * from './types';
export type { GatewayClient } from './client';
export { MockGateway, HumanGateRequiredError } from './mock/MockGateway';
export type { MockGatewayOptions } from './mock/MockGateway';
export { createHttpGateway } from './http/HttpGateway';
export type { HttpGatewayOptions } from './http/HttpGateway';
export { login } from './http/login';
export type { LoginCredentials, LoginResult } from './http/login';

// Per-product gateway slices (one interface + mock + fixtures each). All symbols
// are product-prefixed to avoid collisions across the shared barrel.
export * from './products/forge';
export * from './products/mimir';
export * from './products/mimir-http';
export * from './products/forge-http';
export * from './products/bifrost';
export * from './products/heimdall';
export * from './products/heimdall-http';
export * from './products/admin';
export * from './products/clients';
