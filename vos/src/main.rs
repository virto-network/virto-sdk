use std::{env, error::Error, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // pretty_env_logger::init();

    let home = env::var("VOS_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().map(|d| d.join(".vos")).expect("cwd"));

    vos::start(vos::Cfg {
        uid: &env::var("VOS_BOT").expect("bot user"),
        pwd: &env::var("VOS_BOT_PWD").expect("bot pwd"),
        home,
    })
    .await?;

    Ok(())
}
