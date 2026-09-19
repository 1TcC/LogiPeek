use logipeek::app::settings::{DEFAULT_PRESETS, Settings, Theme};

#[test]
fn default_config_is_stable() {
    let settings = Settings::default();
    assert_eq!(settings.presets, DEFAULT_PRESETS);
    assert_eq!(settings.theme, Theme::System);
}

#[test]
fn valid_config_and_roundtrip_preserve_fields() {
    let settings =
        Settings::parse("preset1=450\npreset2=850\npreset3=1650\npreset4=3250\ntheme=dark\n");
    assert_eq!(settings.presets, [450, 850, 1650, 3250]);
    assert_eq!(settings.theme, Theme::Dark);
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
        };
        assert_eq!(Settings::parse(&settings.serialize()).theme, theme);
    }
}
