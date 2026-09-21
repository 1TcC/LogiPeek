use logipeek::app::{
    settings::Language,
    text::{TextKey, low_battery_message, text},
};

#[test]
fn english_text_mapping_is_stable() {
    assert_eq!(text(Language::English, TextKey::Connected), "Connected");
    assert_eq!(text(Language::English, TextKey::Battery), "Battery");
    assert_eq!(text(Language::English, TextKey::Refresh), "Refresh");
    assert_eq!(
        text(Language::English, TextKey::StartWithWindows),
        "Start with Windows"
    );
    assert_eq!(
        text(Language::English, TextKey::LowBatteryNotifications),
        "Low-battery notifications"
    );
    assert_eq!(
        low_battery_message(Language::English, 18),
        "Mouse battery is low: 18%"
    );
}

#[test]
fn chinese_text_mapping_is_stable() {
    assert_eq!(
        text(Language::SimplifiedChinese, TextKey::Connected),
        "已连接"
    );
    assert_eq!(text(Language::SimplifiedChinese, TextKey::Battery), "电量");
    assert_eq!(text(Language::SimplifiedChinese, TextKey::Refresh), "刷新");
    assert_eq!(
        text(Language::SimplifiedChinese, TextKey::StartWithWindows),
        "开机启动"
    );
    assert_eq!(
        text(
            Language::SimplifiedChinese,
            TextKey::LowBatteryNotifications
        ),
        "低电量通知"
    );
    assert_eq!(
        low_battery_message(Language::SimplifiedChinese, 18),
        "鼠标电量较低：18%"
    );
}
