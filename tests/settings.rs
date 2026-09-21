use logipeek::app::settings::{DEFAULT_PRESETS, Language, Settings, Theme};

#[test]
fn default_config_is_stable() {
    let settings = Settings::default();
    assert_eq!(settings.presets, DEFAULT_PRESETS);
    assert_eq!(settings.theme, Theme::System);
    assert_eq!(settings.language, None);
}

#[test]
fn old_settings_without_language_require_first_run_choice() {
    let settings =
        Settings::parse("preset1=450\npreset2=850\npreset3=1650\npreset4=3250\ntheme=dark\n");
    assert_eq!(settings.presets, [450, 850, 1650, 3250]);
    assert_eq!(settings.theme, Theme::Dark);
    assert_eq!(settings.language, None);
}

#[test]
fn english_language_is_parsed() {
    let settings = Settings::parse("language=en\n");
    assert_eq!(settings.language, Some(Language::English));
    assert!(settings.serialize().contains("language=en\n"));
}

#[test]
fn simplified_chinese_language_is_parsed() {
    let settings = Settings::parse("language=zh-CN\n");
    assert_eq!(settings.language, Some(Language::SimplifiedChinese));
    assert!(settings.serialize().contains("language=zh-CN\n"));
}

#[test]
fn invalid_language_requires_a_new_choice() {
    assert_eq!(Settings::parse("language=fr\n").language, None);
}

#[test]
fn valid_config_and_roundtrip_preserve_fields() {
    let settings = Settings::parse(
        "preset1=450\npreset2=850\npreset3=1650\npreset4=3250\ntheme=dark\nlanguage=zh-CN\n",
    );
    assert_eq!(Settings::parse(&settings.serialize()), settings);
}

#[test]
fn malformed_fields_fall_back_independently_and_unknown_fields_are_ignored() {
    let settings = Settings::parse(
        "preset1=450\npreset2=nope\npreset3=0\npreset4=3250\ntheme=unknown\nfuture=yes\n",
    );
    assert_eq!(settings.presets, [450, 800, 1600, 3250]);
    assert_eq!(settings.theme, Theme::System);
}

#[test]
fn themes_roundtrip() {
    for theme in [Theme::System, Theme::Light, Theme::Dark] {
        let settings = Settings {
            presets: DEFAULT_PRESETS,
            theme,
            language: Some(Language::English),
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&settings.serialize()).theme, theme);
    }
}

#[test]
fn phase_six_defaults_and_roundtrip_are_stable() {
    let defaults = Settings::default();
    assert!(!defaults.startup);
    assert!(defaults.battery_notifications);
    assert_eq!(defaults.battery_threshold, 20);
    assert_eq!(defaults.device, None);

    let parsed = Settings::parse(
        "startup=true\nbattery_notifications=false\nbattery_threshold=35\ndevice=0123abcd\n",
    );
    assert!(parsed.startup);
    assert!(!parsed.battery_notifications);
    assert_eq!(parsed.battery_threshold, 35);
    assert_eq!(parsed.device.as_deref(), Some("0123abcd"));
    assert_eq!(Settings::parse(&parsed.serialize()), parsed);
}

#[test]
fn malformed_phase_six_fields_fall_back_safely() {
    let settings = Settings::parse(
        "startup=yes\nbattery_notifications=1\nbattery_threshold=22\ndevice=raw path!\n",
    );
    assert!(!settings.startup);
    assert!(settings.battery_notifications);
    assert_eq!(settings.battery_threshold, 20);
    assert_eq!(settings.device, None);
}
