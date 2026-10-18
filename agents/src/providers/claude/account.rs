use serde::Deserialize;
use std::{fs, io, path::Path};

#[derive(Deserialize)]
struct Config {
    #[serde(rename = "oauthAccount")]
    oauth_account: Account,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Account {
    pub organization_uuid: String,
    pub account_uuid: String,
}

pub(super) fn read(config_dir: &Path) -> io::Result<Account> {
    let paths = [config_dir.join(".claude.json")]
        .into_iter()
        .chain(config_dir.parent().map(|home| home.join(".claude.json")));
    for path in paths {
        match fs::read(path) {
            Ok(bytes) => return Ok(serde_json::from_slice::<Config>(&bytes)?.oauth_account),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Claude account config not found",
    ))
}
