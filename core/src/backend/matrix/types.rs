use crate::utils::prelude::*;
use crate::base::RegistryState;
use super::prelude::*;

#[derive(Serialize, Clone, Debug, Default, Deserialize, EventContent)]
#[ruma_event(type = "m.virto.apps", kind = GlobalAccountData)]
pub struct MatrixAppsStateContent {
    pub apps: RegistryState,
}
