//! Sway snapshot sent to the frontend.
//!
//! Deliberately composed only of Sway's own reply types (from `swayipc-types`)
//! so the frontend sees Sway's data model, not a webbar-specific one.
//!
//! This file is also compiled by `build.rs` (via `#[path]`) to generate
//! `preload/src/generated/sway.ts`, so it must only depend on `serde` and
//! `swayipc-types`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwayState {
    pub available: bool,
    /// Full layout tree, as returned by `GET_TREE`.
    pub tree: Option<swayipc_types::Node>,
    /// Workspaces, as returned by `GET_WORKSPACES`.
    pub workspaces: Vec<swayipc_types::Workspace>,
    /// Current binding mode, as returned by `GET_BINDING_STATE`.
    pub mode: String,
}
