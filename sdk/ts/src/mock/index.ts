/* @midgard/sdk/mock — the in-memory gateways and their fixtures, for offline
 * demonstrations only.
 *
 * Kept out of the main entry: a console imports this with `await
 * import('@midgard/sdk/mock')` when it runs in mock mode, so the bundler emits it as a
 * separate chunk that an http build never references. Nothing here authenticates.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2] */

export { MockGateway } from './MockGateway';
export type { MockGatewayOptions } from './MockGateway';
export * from './fixtures';
export { AdminMock } from './admin';
export type { AdminMockOptions } from './admin';
export { BifrostMock } from './bifrost';
export type { BifrostMockOptions } from './bifrost';
export { ClientMock } from './clients';
export type { ClientMockOptions } from './clients';
export { ForgeMock } from './forge';
export type { ForgeMockOptions } from './forge';
export { HeimdallMock } from './heimdall';
export type { HeimdallMockOptions } from './heimdall';
export { MimirMock } from './mimir';
export type { MimirMockOptions } from './mimir';
