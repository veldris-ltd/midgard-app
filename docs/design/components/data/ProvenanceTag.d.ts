import * as React from 'react';

/** Provenance chip from the SADs: [DD-4], [NFR-PQ-2], [FR-H-1], [STD:NIST FIPS 203]. Kind is inferred from the id prefix. */
export interface ProvenanceTagProps {
  /** e.g. "DD-4", "NFR-PQ-2", "STD:NIST FIPS 203" (no brackets) */
  id: string;
  /** Override inferred colour family */
  kind?: 'dd' | 'nfr' | 'fr' | 'std' | 'ac' | 'other';
  /** Tooltip text (the requirement statement) */
  title?: string;
  style?: React.CSSProperties;
}
export declare function ProvenanceTag(props: ProvenanceTagProps): React.ReactElement;
