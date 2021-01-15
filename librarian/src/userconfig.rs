use directories_next::UserDirs;
use serde_derive::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const DEFAULT_DIR_NAME: &'static str = "recordplayer";

// configurations to implement
// - file + dir naming (e.g. Artist or Album top level, track name format)
// - how to handle duplicate track entries in the db
//   - replace track file path with new one
//   - don't import new one
//   - let user decide every time

#[derive(Deserialize)]
struct PartialUserConfig {
    pub library_dir: Option<PathBuf>,
    pub copy_on_import: Option<bool>,
}

#[derive(Serialize)]
pub struct UserConfig {
    path: PathBuf,
    library_dir: PathBuf,
    copy_on_import: bool,
}

impl UserConfig {
    /// opens or creates file at path and populates missing properties with
    /// defaults. saves fully populated file before returning
    pub fn load_from(path: PathBuf) -> Self {
        let mut handle = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .unwrap();
        let mut user_config_str = String::new();

        handle.read_to_string(&mut user_config_str).unwrap();

        let PartialUserConfig {
            library_dir,
            copy_on_import,
        } = toml::from_str(&user_config_str).unwrap();

        // config defaults
        let conf = UserConfig {
            path,
            library_dir: library_dir.unwrap_or(
                UserDirs::new()
                    .unwrap()
                    .audio_dir()
                    .unwrap()
                    .join(DEFAULT_DIR_NAME),
            ),
            copy_on_import: copy_on_import.unwrap_or(true),
        };

        conf.save().unwrap();

        conf
    }

    pub fn save(&self) -> std::io::Result<()> {
        let toml_str = toml::to_string_pretty(self).unwrap();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)?;

        file.write_all(toml_str.as_bytes())
    }

    pub fn library_dir(&self) -> &Path {
        self.library_dir.as_path()
    }

    pub fn copy_on_import(&self) -> bool {
        self.copy_on_import
    }
}
