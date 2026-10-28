/* Console navigation model — the full NAV for every product console, ported
 * from docs/design/ui_kits/kit-shell.jsx with hrefs rewritten to in-app router
 * paths. Each product's shell reads its entry: a classification banner, header
 * seals, and sidenav sections whose items carry a stable `id`, an Icon name, a
 * route `href`, and an optional badge. */

import type { ProductId, StatusPillProps } from '@midgard/design-system';
import type { MarkingLevel } from '@midgard/sdk';

/** Console products = the five themed products plus the cross-cutting Admin
 *  panel (which renders under the Midgard red scope with its own ProductMark). */
export type ConsoleProduct = ProductId | 'admin';

export type SealKind = NonNullable<StatusPillProps['status']>;
/** [statusKind, label, pulse] */
export type Seal = [SealKind, string, boolean];

export interface NavItem {
  id: string;
  label: string;
  icon: string;
  href: string;
  badge?: string;
}
export interface NavSection {
  title: string;
  items: NavItem[];
}
export interface NavConfig {
  banner: { level: MarkingLevel; text?: string };
  seals: Seal[];
  sections: NavSection[];
}

export const NAV: Record<ConsoleProduct, NavConfig> = {
  midgard: {
    banner: { level: 'confidential' },
    seals: [['secure', 'PQ-SEALED', true], ['online', 'AIR-GAP', false]],
    sections: [
      { title: 'FABRIC', items: [
        { id: 'objects', label: 'Object types', icon: 'boxes', href: '/' },
        { id: 'search', label: 'Search-around', icon: 'search', href: '/search' },
      ] },
      { title: 'GOVERNANCE', items: [
        { id: 'policies', label: 'Policies & markings', icon: 'shield-check', href: '/policies' },
        { id: 'audit', label: 'Audit lineage', icon: 'scroll-text', href: '/audit' },
      ] },
      { title: 'PLATFORM', items: [
        { id: 'identity', label: 'Identity & access', icon: 'fingerprint', href: '/identity' },
        { id: 'kms', label: 'Keys & crypto', icon: 'key', href: '/kms' },
        { id: 'compute', label: 'Compute', icon: 'server', href: '/compute' },
        { id: 'sdk', label: 'SDK & APIs', icon: 'code', href: '/sdk' },
        { id: 'obs', label: 'Observability', icon: 'activity', href: '/observability' },
        { id: 'resilience', label: 'Resilience', icon: 'refresh-cw', href: '/resilience' },
      ] },
    ],
  },
  forge: {
    banner: { level: 'restricted', text: 'RESTRICTED · DATA PLANE' },
    seals: [['secure', 'PQ-SEALED', false]],
    sections: [
      { title: 'DATA OPS', items: [
        { id: 'pipeline', label: 'Pipelines', icon: 'workflow', href: '/', badge: '38' },
        { id: 'connectors', label: 'Connectors', icon: 'database', href: '/connectors' },
        { id: 'datasets', label: 'Datasets', icon: 'table', href: '/datasets', badge: '412' },
        { id: 'health', label: 'Data health', icon: 'activity', href: '/health', badge: '3' },
        { id: 'schedules', label: 'Schedules', icon: 'clock', href: '/schedules' },
        { id: 'ide', label: 'Transform IDE', icon: 'code', href: '/ide' },
      ] },
      { title: 'FABRIC', items: [
        { id: 'projection', label: 'Object projection', icon: 'boxes', href: '/projection' },
      ] },
    ],
  },
  mimir: {
    banner: { level: 'confidential' },
    seals: [['online', 'INFERENCE · LOCAL', true], ['secure', 'NO EGRESS', false]],
    sections: [
      { title: 'BUILD', items: [
        { id: 'agents', label: 'Agents', icon: 'bot', href: '/', badge: '12' },
        { id: 'logic', label: 'Logic (no-code)', icon: 'workflow', href: '/logic' },
        { id: 'sessions', label: 'Session traces', icon: 'terminal', href: '/sessions' },
      ] },
      { title: 'GOVERN', items: [
        { id: 'evals', label: 'Evals', icon: 'gauge', href: '/evals' },
        { id: 'guardrails', label: 'Guardrails', icon: 'shield-check', href: '/guardrails' },
        { id: 'feedback', label: 'Governed feedback', icon: 'refresh-cw', href: '/feedback' },
      ] },
      { title: 'SERVE', items: [
        { id: 'models', label: 'Model plane', icon: 'cpu', href: '/models', badge: '4' },
      ] },
    ],
  },
  bifrost: {
    banner: { level: 'internal', text: 'INTERNAL · RELEASE CONTROL PLANE' },
    seals: [['secure', 'ARTEFACTS ML-DSA ✓', false]],
    sections: [
      { title: 'DELIVERY', items: [
        { id: 'releases', label: 'Releases', icon: 'rocket', href: '/' },
        { id: 'artefacts', label: 'Artefacts', icon: 'package', href: '/artefacts', badge: '58' },
        { id: 'channels', label: 'Channels', icon: 'git-branch', href: '/channels', badge: '3' },
        { id: 'bundles', label: 'Offline bundles', icon: 'download', href: '/bundles', badge: '2' },
      ] },
      { title: 'FLEET', items: [
        { id: 'fleet', label: 'Fleet & drift', icon: 'server', href: '/fleet' },
        { id: 'rollback', label: 'Rollback centre', icon: 'undo-2', href: '/rollback' },
      ] },
    ],
  },
  heimdall: {
    banner: { level: 'confidential', text: 'CONFIDENTIAL · OPERATIONAL PICTURE · RECIPIENT EYES ONLY' },
    seals: [['online', '12 FEEDS · FUSED', true], ['secure', 'FEEDS PQ-AUTH ✓', false], ['secure', 'SYNC · 3 SITES', false]],
    sections: [
      { title: 'PICTURE', items: [
        { id: 'cop', label: 'Operating picture', icon: 'map', href: '/' },
        { id: 'feeds', label: 'Feeds', icon: 'radio-tower', href: '/feeds', badge: '12' },
        { id: 'entities', label: 'Entities', icon: 'crosshair', href: '/entities', badge: '1,204' },
        { id: 'sync', label: 'Picture sync', icon: 'network', href: '/sync' },
      ] },
      { title: 'DECIDE', items: [
        { id: 'workflows', label: 'Action workflows', icon: 'workflow', href: '/workflows', badge: '2' },
        { id: 'lineage', label: 'Decision lineage', icon: 'scroll-text', href: '/lineage' },
      ] },
    ],
  },
  admin: {
    banner: { level: 'confidential', text: 'CONFIDENTIAL · MANAGEMENT ZONE · ADMIN TRAFFIC SEGREGATED' },
    seals: [['secure', 'SELF-GOVERNED', false], ['online', '7 SUBSYSTEMS', true]],
    sections: [
      { title: 'OVERSEE', items: [
        { id: 'overview', label: 'Overview', icon: 'gauge', href: '/' },
        { id: 'approvals', label: 'Approvals', icon: 'users', href: '/approvals', badge: '3' },
      ] },
      { title: 'ADMINISTER', items: [
        { id: 'iam', label: 'Identity & roles', icon: 'fingerprint', href: '/iam' },
        { id: 'config', label: 'Config & policy', icon: 'git-branch', href: '/config' },
        { id: 'registry', label: 'Registry & governance', icon: 'cpu', href: '/registry' },
        { id: 'ledger', label: 'Audit ledger', icon: 'scroll-text', href: '/ledger' },
      ] },
      { title: 'EMERGENCY', items: [
        { id: 'breakglass', label: 'Break-glass', icon: 'alert-triangle', href: '/breakglass', badge: '1' },
      ] },
      { title: 'ASSIST', items: [
        { id: 'copilot', label: 'Admin copilot', icon: 'sparkles', href: '/copilot' },
      ] },
    ],
  },
};
