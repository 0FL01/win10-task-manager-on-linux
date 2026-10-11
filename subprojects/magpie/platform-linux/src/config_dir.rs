use std::sync::OnceLock;
use std::{env, path::Path};

fn base_path() -> &'static Option<String> {
    static RUNTIME: OnceLock<Option<String>> = OnceLock::new();

    RUNTIME.get_or_init(|| {
        let is_flatpak = {
            if let Ok(v) = env::var("MC_MAGPIE_IS_FLATPAK") {
                v.trim() == "1"
            } else {
                false
            }
        };
        if is_flatpak {
            if let Ok(home) = env::var("HOME") {
                Some(home + "/.var/app/io.missioncenter.MissionCenter/config")
            } else {
                None
            }
        } else {
            if let Ok(value) = env::var("MC_MAGPIE_CONFIG_DIR") {
                Some(value)
            } else if let Ok(xdg_config_dir) = env::var("XDG_CONFIG_HOME") {
                Some(xdg_config_dir + "/missioncenter")
            } else if let Ok(home) = env::var("HOME") {
                Some(home + "/.config/missioncenter")
            } else {
                None
            }
        }
    })
}

pub fn build_path(appendix: &str) -> Option<Box<Path>> {
    if let Some(base_path) = base_path() {
        let string = format!("{}/{}", base_path, appendix);
        let path = Path::new(&string);
        if path.exists() {
            Some(path.into())
        } else {
            None
        }
    } else {
        None
    }
}
