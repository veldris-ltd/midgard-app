//! Picture layers. A layer groups entities of a kind so an operator can toggle a
//! whole class of contacts in or out of the fused view. [FR-H-1]

use crate::id::LayerId;

/// A named layer of the operating picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// Stable layer id.
    pub id: LayerId,
    /// Human-readable name.
    pub name: String,
}

impl Layer {
    /// Construct a layer.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: LayerId(id.into()),
            name: name.into(),
        }
    }
}
