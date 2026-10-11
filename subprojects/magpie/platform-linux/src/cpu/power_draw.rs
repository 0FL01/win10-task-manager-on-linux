use std::{fs::read_to_string, path::PathBuf, time::Instant};

pub struct PowerDraw {
    path: PathBuf,
    max_uj: Option<u64>,
    last_uj: u64,
    time_last: Instant,
}

impl PowerDraw {
    pub fn default() -> Option<Self> {
        let path: Option<(PathBuf, u64)> = {
            let dir = match std::fs::read_dir("/sys/class/powercap") {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("Failed to open `/sys/class/powercap`: {e}");
                    return None;
                }
            };

            let mut out = None;

            for mut entry in dir
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|path| path.is_dir())
            {
                let mut name = entry.clone();
                name.push("name");
                let name = match std::fs::read_to_string(name) {
                    Ok(name) => name.trim().to_ascii_lowercase(),
                    Err(_) => continue,
                };
                if name.starts_with("package") {
                    entry.push("energy_uj");
                    match read_to_string(&entry) {
                        Ok(uj) => match uj.trim().parse::<u64>() {
                            Ok(power_draw) => out = Some((entry, power_draw)),
                            Err(e) => {
                                log::warn!(
                                    "Failed to parse power drawn from `{}`, skipping: {e}",
                                    entry.display()
                                );
                            }
                        },

                        Err(e) => {
                            log::warn!(
                                "Failed to read power drawn from `{}`, skipping: {e}",
                                entry.display()
                            );
                        }
                    }
                }
            }
            out
        };
        let (path, last_uj) = path?;

        let mut max_uj = None;
        let mut power_draw_max_path = path.parent()?.to_path_buf();
        power_draw_max_path.push("max_energy_range_uj");

        match read_to_string(&power_draw_max_path) {
            Ok(uj) => match uj.trim().parse::<u64>() {
                Ok(max_uj_t) => max_uj = Some(max_uj_t),
                Err(e) => {
                    log::warn!(
                        "Failed to parse power drawn max from `{}`: {e}",
                        power_draw_max_path.display()
                    );
                }
            },
            Err(e) => {
                log::warn!(
                    "Failed to read power drawn max from `{}`: {e}",
                    power_draw_max_path.display()
                );
            }
        };

        Some(Self {
            path,
            max_uj,
            last_uj,
            time_last: Instant::now(),
        })
    }
}

impl PowerDraw {
    pub fn update(&mut self) -> Option<f32> {
        let cur_uj = match std::fs::read_to_string(&self.path) {
            Ok(uj) => match uj.trim().parse::<u64>() {
                Ok(cur_uj) => cur_uj,
                Err(e) => {
                    log::warn!(
                        "Failed to parse power draw from `{}`: {e}",
                        &self.path.display()
                    );
                    return None;
                }
            },
            Err(e) => {
                log::warn!(
                    "Failed to read power drawn from `{}`: {e}",
                    &self.path.display()
                );
                return None;
            }
        };
        let diff_uj = {
            if self.last_uj > cur_uj {
                (self.max_uj.unwrap_or(self.last_uj) - self.last_uj) + cur_uj
            } else {
                cur_uj - self.last_uj
            }
        };
        let elapsed = self.time_last.elapsed().as_secs_f32();
        self.time_last = Instant::now();
        let w = (diff_uj as f32) / (elapsed * 1_000_000.);
        self.last_uj = cur_uj;
        Some(w)
    }
}
