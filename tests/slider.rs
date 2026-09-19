use logipeek::{app::slider, hid::features::dpi::DpiValues};

#[test]
fn range_maps_minimum_maximum_and_snaps_ties_higher() {
    let values = DpiValues::Range {
        minimum: 100,
        maximum: 1600,
        step: 50,
    };
    assert_eq!(slider::value_at(&values, 0, 300), Some(100));
    assert_eq!(slider::value_at(&values, 300, 300), Some(1600));
    assert_eq!(slider::value_at(&values, 5, 300), Some(150));
    assert_eq!(slider::position_of(&values, 850, 300), Some(150));
}

#[test]
fn discrete_slider_uses_indices_instead_of_inventing_values() {
    let values = DpiValues::List(vec![400, 800, 1600, 3200]);
    assert_eq!(slider::value_at(&values, 0, 300), Some(400));
    assert_eq!(slider::value_at(&values, 100, 300), Some(800));
    assert_eq!(slider::value_at(&values, 200, 300), Some(1600));
    assert_eq!(slider::value_at(&values, 300, 300), Some(3200));
    assert_eq!(slider::position_of(&values, 1600, 300), Some(200));
    assert_eq!(slider::position_of(&values, 1300, 300), None);
}

#[test]
fn extreme_widths_and_invalid_values_are_safe() {
    let range = DpiValues::Range {
        minimum: 400,
        maximum: 1600,
        step: 100,
    };
    assert_eq!(slider::value_at(&range, 99, 0), Some(400));
    assert_eq!(slider::position_of(&range, 800, -1), None);
    let invalid = DpiValues::Range {
        minimum: 400,
        maximum: 1600,
        step: 0,
    };
    assert_eq!(slider::value_at(&invalid, 5, 10), None);
    assert_eq!(slider::position_of(&invalid, 800, 100), None);
}
