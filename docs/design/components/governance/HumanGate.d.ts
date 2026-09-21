import * as React from 'react';

/** Mandatory authorisation card for state-mutating AI proposals (NFR-AI-1). Amber pending; green authorised with audit id; neutral rejected. */
export interface HumanGateProps {
  /** The proposed action, e.g. "Schedule maintenance · AC-2214 → FAC-DUB" */
  title?: string;
  /** Override the default HUMAN GATE REQUIRED line */
  detail?: string;
  state?: 'pending' | 'authorised' | 'rejected';
  /** Audit id shown after resolution, e.g. "A-77120" */
  auditId?: string;
  onAuthorise?: () => void;
  onReject?: () => void;
  style?: React.CSSProperties;
}
export declare function HumanGate(props: HumanGateProps): React.ReactElement;
