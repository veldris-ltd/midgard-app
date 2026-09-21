import * as React from 'react';

/** Dual-control approval card (FR-AD-5): risk bars, mandatory justification, requester/approver with live separation-of-duties check, time-boxed single-use token on approval. */
export interface GuardedActionProps {
  action: string;
  subsystem?: string;
  requester?: string;
  approver?: string;
  risk?: 1 | 2 | 3 | 4 | 5;
  state?: 'awaiting' | 'approved' | 'executed' | 'denied';
  justification?: string;
  /** Shown when state='denied', e.g. "DENIED · SELF-APPROVAL (AC-AD-4)" */
  deniedReason?: string;
  /** Token TTL label on approval. Default "10m" */
  tokenTtl?: string;
  onApprove?: () => void;
  onDeny?: () => void;
  style?: React.CSSProperties;
}
export declare function GuardedAction(props: GuardedActionProps): React.ReactElement;
