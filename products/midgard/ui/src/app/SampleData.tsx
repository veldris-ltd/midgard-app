import { Badge } from '@midgard/design-system';

/* SampleData — marks a page whose figures are fixed examples, not this enclosure's.
 * The gateway serves identities, keys, compute, observability, resilience and the SDK
 * surface from fixed data (services/http-gateway/src/data.rs) until those subsystems
 * are wired to it, so these pages say so rather than pass the examples off as live.
 * [AC-M-4] */
export function SampleData() {
  return (
    <span title="Fixed example data served by the gateway, not this enclosure's own. The subsystem behind this page is not connected yet.">
      <Badge tone="warn">SAMPLE DATA</Badge>
    </span>
  );
}
