use crate::VSupervisor;
use crate::{utils::HashMap, VRunnable};
use serde::{de::DeserializeOwned, Serialize};

struct MatrixSupervisor {
    apps: HashMap<String, Box<dyn VRunnable>>,
}

// impl VSupervisor for MatrixSupervisor {
//     // fn add<To>(app_id: &str, app: Box<dyn VRunnable<To>>) {}

//     // fn run<Command: DeserializeOwned + Serialize + Send>(cmd: CommandEvelope<Command>) {}
// }
