import * as React from 'react';
import type { AuditRecord, GatewayClient } from '@midgard/sdk';

/* The data seam, injected once at the app root. Screens read the client via
 * `useGateway()` and declare their data needs with `useAsync` / `useAuditStream`
 * — they never know whether the source is the mock or a real HTTP bridge. */

const GatewayContext = React.createContext<unknown>(null);

/** Inject a product's gateway at the app root. `client` is any GatewayClient
 *  variant (Midgard, Forge, …); screens read it back with `useGateway<T>()`. */
export function DataProvider({ client, children }: { client: unknown; children: React.ReactNode }) {
  return <GatewayContext.Provider value={client}>{children}</GatewayContext.Provider>;
}

/** Read the injected gateway, typed to the product's client interface.
 *  Defaults to the Midgard `GatewayClient` so existing screens are unchanged. */
export function useGateway<T = GatewayClient>(): T {
  const c = React.useContext(GatewayContext);
  if (!c) throw new Error('useGateway must be used within a <DataProvider>.');
  return c as T;
}

export interface AsyncState<T> {
  data: T | null;
  loading: boolean;
  error: Error | null;
  /** Re-run the loader. */
  reload: () => void;
}

/** Run an async loader against the gateway, with loading/error state and reload.
 *  `C` defaults to the Midgard client; product screens pass their own gateway
 *  type, e.g. `useAsync<Pipeline[], ForgeGateway>((c) => c.listPipelines())`. */
export function useAsync<T, C = GatewayClient>(loader: (client: C) => Promise<T>, deps: React.DependencyList = []): AsyncState<T> {
  const client = useGateway<C>();
  const [data, setData] = React.useState<T | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<Error | null>(null);
  const [nonce, setNonce] = React.useState(0);

  React.useEffect(() => {
    let live = true;
    setLoading(true);
    setError(null);
    loader(client)
      .then((d) => {
        if (live) {
          setData(d);
          setLoading(false);
        }
      })
      .catch((e) => {
        if (live) {
          setError(e instanceof Error ? e : new Error(String(e)));
          setLoading(false);
        }
      });
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client, nonce, ...deps]);

  const reload = React.useCallback(() => setNonce((n) => n + 1), []);
  return { data, loading, error, reload };
}

/** Subscribe to the live audit lineage, keeping the most recent `max` records.
 *  Works with any gateway that exposes `auditStream(cb)`. */
export function useAuditStream(seed: AuditRecord[] = [], max = 60): AuditRecord[] {
  const client = useGateway<{ auditStream(cb: (r: AuditRecord) => void): () => void }>();
  const [records, setRecords] = React.useState<AuditRecord[]>(seed);

  // Keep in sync when a new seed arrives (e.g. after the initial fetch resolves).
  const seedKey = seed.map((r) => r.id).join(',');
  React.useEffect(() => {
    if (seed.length) setRecords(seed);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seedKey]);

  React.useEffect(() => {
    const off = client.auditStream((rec) => {
      setRecords((prev) => [rec, ...prev].slice(0, max));
    });
    return off;
  }, [client, max]);

  return records;
}
