//! The resource-id scheme for policy decisions on fabric data, defined once and used
//! by the fabric and the gateway alike, so a rule written against a type or an object
//! means the same thing on every route. [VLD-ANB-MIDGARD-0023] [NFR-SEC-2]
//!
//! - `object-type:<type>` names an object type: a `/query` or `/search` of that
//!   type, and a search-around from every object of it (`<type>/*`).
//! - `object:<type>/<key>` names one object (its id, prefixed): a search-around from
//!   that seed, and the per-object read check that filters query and search results,
//!   search-around nodes and action targets.
//!
//! The per-object check consults both ids, so a deny on `object-type:aircraft` hides
//! every aircraft wherever it would appear, a deny on `object:aircraft/AC-1` hides
//! that one aircraft, and a permit on either is enough when nothing denies. Route
//! resources (literal paths such as `/api/v1/audit`) are not part of this scheme.

use std::collections::BTreeSet;

use audit_writer::LedgerStore;
use governance::{GovContext, Governance};
use policy::{
    ClassificationLevel, Decision, DecisionReason, Marking, Purpose, Resource, ResourceId,
};

use crate::object::ObjectInstance;

/// The prefix of a type-level resource id.
pub const TYPE_PREFIX: &str = "object-type:";
/// The prefix of an object-level resource id.
pub const OBJECT_PREFIX: &str = "object:";

/// The resource id of object type `type_id`: `object-type:<type>`.
pub fn type_resource_id(type_id: &str) -> ResourceId {
    ResourceId(format!("{TYPE_PREFIX}{type_id}"))
}

/// The resource id of the object whose id is `object_id` (`<type>/<key>`):
/// `object:<type>/<key>`.
pub fn object_resource_id(object_id: &str) -> ResourceId {
    ResourceId(format!("{OBJECT_PREFIX}{object_id}"))
}

/// The type-level resource a query, search or wildcard search-around is authorised
/// on: UNCLASSIFIED, for the caller's purpose. The objects it returns are each
/// marking-checked on their own.
pub fn type_resource(type_id: &str, purpose: &Purpose) -> Resource {
    Resource {
        id: type_resource_id(type_id),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [purpose.clone()].into_iter().collect(),
    }
}

/// The resource for one object instance: its scheme id, marking and purposes.
pub fn object_resource(obj: &ObjectInstance) -> Resource {
    Resource {
        id: object_resource_id(&obj.id),
        marking: obj.marking.clone(),
        allowed_purposes: obj.allowed_purposes.clone(),
    }
}

/// The per-object read check, through the governance point's non-auditing fast path:
/// mandatory marking dominance and the purpose gate on the object's own marking, then
/// the role rules on both the object's id and its type's id. An explicit deny on
/// either refuses it; otherwise a permit on either grants it.
pub fn can_read_object<L: LedgerStore>(
    gov: &Governance<L>,
    ctx: &GovContext,
    obj: &ObjectInstance,
) -> bool {
    let instance = gov.decide(ctx, &object_resource(obj), "Read", &ctx.purpose);
    let of_type = Resource {
        id: type_resource_id(&obj.type_id),
        ..object_resource(obj)
    };
    let of_type = gov.decide(ctx, &of_type, "Read", &ctx.purpose);
    let explicitly_denied = |d: &Decision| {
        d.reasons
            .iter()
            .any(|r| matches!(r, DecisionReason::ExplicitDeny { .. }))
    };
    if explicitly_denied(&instance) || explicitly_denied(&of_type) {
        return false;
    }
    instance.is_permit() || of_type.is_permit()
}
