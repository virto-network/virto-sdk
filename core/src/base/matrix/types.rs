use serde::{Deserialize, Serialize};

use super::super::types::VAppsState;
use matrix_sdk::ruma::events::macros::EventContent;

#[derive(Serialize, Clone, Debug, Default, Deserialize, EventContent)]
#[ruma_event(type = "m.virto.apps", kind = GlobalAccountData)]
pub struct MatrixAppsStateContent {
    pub apps: VAppsState,
}
