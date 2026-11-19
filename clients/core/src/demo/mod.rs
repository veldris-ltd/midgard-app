//! The demo enclosure: a signed, bundled dataset served behind the same data-source
//! interface a gateway connection implements. `[AC-C-13]` `[NFR-PQ-3]`

pub mod audit;
pub mod build;
pub mod canon;
pub mod dataset;
pub mod enclosure;
pub mod guard;
