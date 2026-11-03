//! The policy context a mesh service authorises a request under, taken from what the
//! channel authorised the peer for: its certified roles, and the clearance its
//! certificate grants (capped by the posture it asked for), never a level the service
//! assumes. Every mesh service builds its per-request context here, so none of them
//! hand-builds a clearance. [NFR-SEC-2] [VLD-ANB-MIDGARD-0063]
//!
//! A resource's marking is configuration (`--resource-marking`, see
//! [`crate::marking::parse_marking`]); the default, open with no compartments, is what
//! every mesh resource was before.

use governance::GovContext;
use policy::{Clearance, Purpose};
use pq_mtls::{Authorization, WorkloadIdentity};

/// The context a request from `peer` is authorised and audited under, for `purpose`:
/// the subject and roles from its certificate, the clearance the channel authorised it
/// at. [VLD-ANB-MIDGARD-0063]
pub fn mesh_context(
    peer: &WorkloadIdentity,
    authorization: &Authorization,
    purpose: &Purpose,
) -> GovContext {
    GovContext {
        subject: peer.id.0.clone(),
        roles: peer.roles.clone(),
        clearance: certified_clearance(authorization),
        purpose: purpose.clone(),
    }
}

/// The clearance the channel authorised its peer at, as a policy clearance.
pub fn certified_clearance(authorization: &Authorization) -> Clearance {
    Clearance {
        level: authorization.clearance.level,
        compartments: authorization.clearance.compartments.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use policy::{ClassificationLevel, Compartment, Role};
    use pq_mtls::{Posture, ServiceId};
    use pqc_agility::{SigId, VerifyingKey};

    use super::*;

    #[test]
    fn mesh_context_takes_clearance_from_the_certificate() {
        // [VLD-ANB-MIDGARD-0063] [AC-22.4]
        let peer = WorkloadIdentity {
            id: ServiceId("feed-source-1".into()),
            roles: [Role("feed-source".into())].into_iter().collect(),
            sig_alg: SigId::MlDsa65,
            verifying_key: VerifyingKey::new(SigId::MlDsa65, vec![1; 32]),
        };
        let compartments: BTreeSet<Compartment> =
            [Compartment("OPS-N".into())].into_iter().collect();
        let authorization = Authorization {
            clearance: Posture {
                level: ClassificationLevel::SECRET,
                compartments: compartments.clone(),
            },
            purpose: Purpose("situational-awareness".into()),
            purposes: Some(
                [Purpose("situational-awareness".into())]
                    .into_iter()
                    .collect(),
            ),
        };
        let ctx = mesh_context(
            &peer,
            &authorization,
            &Purpose("situational-awareness".into()),
        );
        assert_eq!(ctx.subject, "feed-source-1");
        assert_eq!(ctx.roles, peer.roles);
        assert_eq!(ctx.clearance.level, ClassificationLevel::SECRET);
        assert_eq!(ctx.clearance.compartments, compartments);
        assert_eq!(ctx.purpose, Purpose("situational-awareness".into()));
    }
}
