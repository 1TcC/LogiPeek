use logipeek::app::{
    settings::Language,
    text::{TextKey, text},
};

#[test]
fn english_text_mapping_is_stable() {
    assert_eq!(text(Language::English, TextKey::Connected), "Connected");
    assert_eq!(text(Language::English, TextKey::Battery), "Battery");
    assert_eq!(text(Language::English, TextKey::Refresh), "Refresh");
}

#[test]
fn chinese_text_mapping_is_stable() {
    assert_eq!(
        text(Language::SimplifiedChinese, TextKey::Connected),
        "已连接"
    );
    assert_eq!(text(Language::SimplifiedChinese, TextKey::Battery), "电量");
    assert_eq!(text(Language::SimplifiedChinese, TextKey::Refresh), "刷新");
}
