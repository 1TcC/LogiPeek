use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const DEFAULT_PRESETS: [u16; 4] = [400, 800, 1600, 3200];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    SimplifiedChinese,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub presets: [u16; 4],
    pub theme: Theme,
    pub language: Option<Language>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            presets: DEFAULT_PRESETS,
            theme: Theme::System,
            language: None,
        }
    }
}

impl Settings {
    pub fn parse(input: &str) -> Self {
        let defaults = Self::default();
        let mut settings = defaults.clone();
        for line in input.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "preset1" => settings.presets[0] = parse_preset(value, defaults.presets[0]),
                "preset2" => settings.presets[1] = parse_preset(value, defaults.presets[1]),
                "preset3" => settings.presets[2] = parse_preset(value, defaults.presets[2]),
                "preset4" => settings.presets[3] = parse_preset(value, defaults.presets[3]),
                "theme" => {
                    settings.theme = match value {
                        "system" => Theme::System,
                        "light" => Theme::Light,
                        "dark" => Theme::Dark,
                        _ => defaults.theme,
                    };
                }
                "language" => {
                    settings.language = match value {
                        "en" => Some(Language::English),
                        "zh-CN" => Some(Language::SimplifiedChinese),
                        _ => None,
                    };
                }
                _ => {}
            }
        }
        settings
    }

    pub fn serialize(&self) -> String {
        let theme = match self.theme {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        };
        let language = match self.language {
            Some(Language::English) => "language=en\n",
            Some(Language::SimplifiedChinese) => "language=zh-CN\n",
            None => "",
        };
        format!(
            "preset1={}\npreset2={}\npreset3={}\npreset4={}\ntheme={theme}\n{language}",
            self.presets[0], self.presets[1], self.presets[2], self.presets[3]
        )
    }

    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .map(|contents| Self::parse(&contents))
            .unwrap_or_default()
    }

    /// Writes and syncs a complete sibling temporary file. The Windows UI
    /// layer then atomically replaces the destination with MoveFileExW.
    pub fn write_temporary(&self, path: &Path) -> io::Result<PathBuf> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        let mut file = File::create(&temporary)?;
        file.write_all(self.serialize().as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        Ok(temporary)
    }
}

fn parse_preset(value: &str, fallback: u16) -> u16 {
    value
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}
