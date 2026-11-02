/* Errors the gateway clients throw. Shared by the HTTP client and the mock, outside
 * src/mock/ so the main entry does not reach the mock for them. [VLD-ANB-MIDGARD-0086]
 * [AC-25.2] */

/** Error thrown when a gated action is applied without a current human-gate
 *  approval. `message` is the gateway's own wording, where it gave one. */
export class HumanGateRequiredError extends Error {
  constructor(actionType: string, message?: string) {
    super(message ?? `Human gate required for action "${actionType}"`);
    this.name = 'HumanGateRequiredError';
  }
}

/** A request the gateway refused, with its HTTP status and the gateway's message. */
export class GatewayError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = 'GatewayError';
    this.status = status;
  }
}
