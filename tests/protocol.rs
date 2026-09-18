use logipeek::hid::{
    device::{self, Endpoint, FeatureResult},
    features::battery::{self, Charging, Level},
    hidpp::{self, Error, Exchange, Feature, Packet, Protocol},
    transport::parse_input,
};
use std::collections::VecDeque;

#[test]
fn packet_parse_requires_exact_report_lengths() {
    let short = [0x10, 1, 2, 3, 4, 5];
    let long = [0x11; 21];
    let valid_short = [0x10, 1, 2, 3, 4, 5, 6];
    let valid_long = [0x11; 20];
    assert_eq!(
        Packet::parse(&short),
        Err(Error::Malformed("incorrect report length"))
    );
    assert_eq!(
        Packet::parse(&long),
        Err(Error::Malformed("incorrect report length"))
    );
    assert_eq!(
        Packet::parse(&[0x12; 7]),
        Err(Error::Malformed("unsupported report ID"))
    );
    assert_eq!(
        Packet::parse(&valid_short).unwrap().parameters,
        vec![4, 5, 6]
    );
    assert_eq!(Packet::parse(&valid_long).unwrap().parameters.len(), 16);
}

#[test]
fn packet_parse_rejects_empty_and_all_short_prefixes_without_panicking() {
    assert_eq!(
        Packet::parse(&[]),
        Err(Error::Malformed("unsupported report ID"))
    );
    for length in 1..7 {
        let mut bytes = vec![0x10; length];
        if let Some(first) = bytes.first_mut() {
            *first = 0x10;
        }
        assert_eq!(
            Packet::parse(&bytes),
            Err(Error::Malformed("incorrect report length"))
        );
    }
}

#[test]
fn parse_input_accepts_native_report_sizes_and_zero_padding() {
    let short = [0x10, 1, 2, 3, 4, 5, 6];
    let long = [0x11; 20];
    assert_eq!(parse_input(&short).unwrap().parameters, vec![4, 5, 6]);
    assert_eq!(parse_input(&long).unwrap().parameters.len(), 16);

    let mut padded_short = vec![0x10, 1, 2, 3, 4, 5, 6];
    padded_short.resize(20, 0);
    assert_eq!(parse_input(&padded_short).unwrap().device, 1);
    padded_short.resize(64, 0);
    assert_eq!(parse_input(&padded_short).unwrap().feature, 2);

    let mut padded_long = long.to_vec();
    padded_long.resize(64, 0);
    assert_eq!(parse_input(&padded_long).unwrap().report, 0x11);
}

#[test]
fn parse_input_rejects_truncated_nonzero_padding_and_overlong_reports() {
    let truncated_short = [0x10, 1, 2, 3, 4, 5];
    let truncated_long = [0x11; 19];
    assert_eq!(
        parse_input(&truncated_short),
        Err(Error::Malformed("invalid HID report padding or length"))
    );
    assert_eq!(
        parse_input(&truncated_long),
        Err(Error::Malformed("invalid HID report padding or length"))
    );

    let mut nonzero_padding = vec![0x10, 1, 2, 3, 4, 5, 6];
    nonzero_padding.resize(20, 0);
    nonzero_padding[19] = 1;
    assert_eq!(
        parse_input(&nonzero_padding),
        Err(Error::Malformed("invalid HID report padding or length"))
    );

    let mut overlong = vec![0x11; 65];
    overlong[20..].fill(0);
    assert_eq!(
        parse_input(&overlong),
        Err(Error::Malformed("invalid HID report padding or length"))
    );
}

#[test]
fn packet_response_ignores_unknown_device_software_id_and_feature() {
    let request = [0x10, 0x02, 0x20, 0x05, 0xaa];
    for packet in [
        Packet {
            report: 0x10,
            device: 0x03,
            feature: 0x20,
            function: 0x05,
            parameters: vec![1],
        },
        Packet {
            report: 0x10,
            device: 0x02,
            feature: 0x20,
            function: 0x06,
            parameters: vec![1],
        },
        Packet {
            report: 0x10,
            device: 0x02,
            feature: 0x21,
            function: 0x05,
            parameters: vec![1],
        },
    ] {
        assert_eq!(packet.response_to(&request), None);
    }
}

#[test]
fn packet_response_matches_success_and_protocol_errors() {
    let request = [0x10, 0x02, 0x20, 0x05];
    let success = Packet {
        report: 0x10,
        device: 0x02,
        feature: 0x20,
        function: 0x05,
        parameters: vec![9, 8],
    };
    assert_eq!(success.response_to(&request), Some(Ok(vec![9, 8])));
    for (feature, legacy) in [(0x8f, true), (0xff, false)] {
        let error = Packet {
            report: 0x10,
            device: 0x02,
            feature,
            function: 0x20,
            parameters: vec![0x05, 0x07],
        };
        assert_eq!(
            error.response_to(&request),
            Some(Err(Error::Protocol { legacy, code: 0x07 }))
        );
    }
    let truncated = Packet {
        report: 0x10,
        device: 0x02,
        feature: 0xff,
        function: 0x20,
        parameters: vec![0x05],
    };
    assert_eq!(
        truncated.response_to(&request),
        Some(Err(Error::Malformed("truncated protocol error")))
    );
}

#[test]
fn feature_parse_reports_unsupported_and_truncated_data() {
    assert_eq!(Feature::parse(0x1000, &[0, 0, 0]).unwrap(), None);
    assert_eq!(
        Feature::parse(0, &[0, 0, 0]).unwrap(),
        Some(Feature {
            id: 0,
            index: 0,
            flags: 0,
            version: 0
        })
    );
    assert_eq!(
        Feature::parse(0x1000, &[1, 2]),
        Err(Error::Malformed("truncated feature response"))
    );
}

#[derive(Debug)]
struct FakeExchange {
    result: Result<Vec<u8>, Error>,
    calls: Vec<(u8, u8, u8, Vec<u8>)>,
}
impl FakeExchange {
    fn returning(result: Result<Vec<u8>, Error>) -> Self {
        Self {
            result,
            calls: Vec::new(),
        }
    }
}
impl Exchange for FakeExchange {
    fn exchange(
        &mut self,
        device: u8,
        feature: u8,
        function: u8,
        parameters: &[u8],
    ) -> Result<Vec<u8>, Error> {
        self.calls
            .push((device, feature, function, parameters.to_vec()));
        self.result.clone()
    }
}

#[derive(Debug)]
struct QueueExchange {
    replies: VecDeque<Result<Vec<u8>, Error>>,
    calls: Vec<(u8, u8, u8, Vec<u8>)>,
}

impl QueueExchange {
    fn new(replies: impl IntoIterator<Item = Result<Vec<u8>, Error>>) -> Self {
        Self {
            replies: replies.into_iter().collect(),
            calls: Vec::new(),
        }
    }
}

impl Exchange for QueueExchange {
    fn exchange(
        &mut self,
        device: u8,
        feature: u8,
        function: u8,
        parameters: &[u8],
    ) -> Result<Vec<u8>, Error> {
        self.calls
            .push((device, feature, function, parameters.to_vec()));
        self.replies.pop_front().unwrap_or(Err(Error::Timeout))
    }
}

#[test]
fn probe_validates_ping_and_protocol_version_and_recognizes_legacy() {
    let mut feature = FakeExchange::returning(Ok(vec![2, 1, 0xa5]));
    assert_eq!(
        hidpp::probe(&mut feature, 0xff),
        Ok(Protocol::Feature { major: 2, minor: 1 })
    );
    assert_eq!(feature.calls, vec![(0xff, 0, 1, vec![0, 0, 0xa5])]);
    let mut bad_ping = FakeExchange::returning(Ok(vec![2, 1, 0xa4]));
    assert_eq!(
        hidpp::probe(&mut bad_ping, 1),
        Err(Error::Malformed("invalid protocol version or ping echo"))
    );
    let mut bad_version = FakeExchange::returning(Ok(vec![1, 1, 0xa5]));
    assert_eq!(
        hidpp::probe(&mut bad_version, 1),
        Err(Error::Malformed("invalid protocol version or ping echo"))
    );
    let mut legacy = FakeExchange::returning(Err(Error::Protocol {
        legacy: true,
        code: 1,
    }));
    assert_eq!(hidpp::probe(&mut legacy, 0xff), Ok(Protocol::Legacy));
    let mut wrong_error = FakeExchange::returning(Err(Error::Protocol {
        legacy: true,
        code: 2,
    }));
    assert_eq!(
        hidpp::probe(&mut wrong_error, 0xff),
        Err(Error::Protocol {
            legacy: true,
            code: 2
        })
    );
}

#[test]
fn discover_sends_feature_id_in_big_endian_order() {
    let mut fake = FakeExchange::returning(Ok(vec![7, 0x03, 2]));
    assert_eq!(
        hidpp::discover(&mut fake, 0x04, 0x1234).unwrap(),
        Some(Feature {
            id: 0x1234,
            index: 7,
            flags: 3,
            version: 2
        })
    );
    assert_eq!(fake.calls, vec![(0x04, 0, 0, vec![0x12, 0x34])]);
}

#[test]
fn battery_parsing_distinguishes_coarse_and_percentage_levels() {
    let coarse = battery::parse_1000(&[9, 2], &[80, 20, 0]).unwrap();
    assert_eq!(coarse.level, Level::Good);
    assert_eq!(coarse.charging, Charging::Discharging);
    let percentage = battery::parse_1000(&[10, 2], &[80, 20, 1]).unwrap();
    assert_eq!(percentage.level, Level::Percentage(80));
    assert_eq!(percentage.charging, Charging::Charging);
}

#[test]
fn battery_zero_is_unknown_and_invalid_ranges_are_rejected() {
    assert_eq!(
        battery::parse_1000(&[10, 2], &[0, 0, 3]).unwrap().level,
        Level::Unknown
    );
    for (capabilities, status) in [
        (&[1, 0][..], &[50, 0, 0][..]),
        (&[10, 0][..], &[101, 0, 0][..]),
        (&[10, 0][..], &[50, 51, 0][..]),
    ] {
        assert_eq!(
            battery::parse_1000(capabilities, status),
            Err(Error::Malformed("invalid battery level"))
        );
    }
    assert_eq!(
        battery::parse_1000(&[10, 0], &[50, 0, 9]).unwrap().charging,
        Charging::Unknown(9)
    );
}

#[test]
fn battery_read_uses_dynamic_index_and_queries_capabilities_before_status() {
    let feature = Feature {
        id: 0x1000,
        index: 7,
        flags: 0,
        version: 0,
    };
    let mut exchange = QueueExchange::new([Ok(vec![100, 2, 0]), Ok(vec![74, 0, 1])]);
    let battery = battery::read(&mut exchange, 3, feature).unwrap();
    assert_eq!(battery.level, Level::Percentage(74));
    assert_eq!(battery.charging, Charging::Charging);
    assert_eq!(exchange.calls, vec![(3, 7, 1, vec![]), (3, 7, 0, vec![])]);
}

#[test]
fn battery_read_rejects_unsupported_feature_without_exchange() {
    for feature in [
        Feature {
            id: 0x1001,
            index: 7,
            flags: 0,
            version: 0,
        },
        Feature {
            id: 0x1000,
            index: 7,
            flags: 0,
            version: 1,
        },
    ] {
        let mut exchange = QueueExchange::new(Vec::<Result<Vec<u8>, Error>>::new());
        assert_eq!(
            battery::read(&mut exchange, 3, feature),
            Err(Error::Malformed("battery feature version not implemented"))
        );
        assert!(exchange.calls.is_empty());
    }
}

#[test]
fn battery_read_propagates_capability_error_and_skips_status() {
    let feature = Feature {
        id: 0x1000,
        index: 7,
        flags: 0,
        version: 0,
    };
    let mut exchange = QueueExchange::new([Err(Error::Io), Ok(vec![74, 0, 1])]);
    assert_eq!(battery::read(&mut exchange, 3, feature), Err(Error::Io));
    assert_eq!(exchange.calls, vec![(3, 7, 1, vec![])]);
}

fn endpoint_with_features(features: Vec<FeatureResult>) -> Endpoint {
    Endpoint {
        index: 1,
        protocol: Ok(Protocol::Feature { major: 2, minor: 0 }),
        features,
        battery: None,
    }
}

#[test]
fn capability_distinguishes_supported_unsupported_and_unknown() {
    let supported = endpoint_with_features(vec![FeatureResult {
        id: 0x1000,
        result: Ok(Some(Feature {
            id: 0x1000,
            index: 4,
            flags: 0,
            version: 0,
        })),
    }]);
    assert_eq!(
        device::capability(&supported, &[0x1000, 0x1001]),
        "Supported"
    );

    let unsupported = endpoint_with_features(vec![
        FeatureResult {
            id: 0x1000,
            result: Ok(None),
        },
        FeatureResult {
            id: 0x1001,
            result: Ok(None),
        },
    ]);
    assert_eq!(
        device::capability(&unsupported, &[0x1000, 0x1001]),
        "Unsupported (queried features)"
    );

    let partly_failed = endpoint_with_features(vec![
        FeatureResult {
            id: 0x1000,
            result: Ok(None),
        },
        FeatureResult {
            id: 0x1001,
            result: Err(Error::Io),
        },
    ]);
    assert_eq!(
        device::capability(&partly_failed, &[0x1000, 0x1001]),
        "Unknown"
    );
}

#[test]
fn safe_label_removes_controls_and_caps_length() {
    let input = format!("prefix\n\t{}suffix", "x".repeat(120));
    let output = device::safe_label(&input);
    assert!(!output.chars().any(char::is_control));
    assert_eq!(output.chars().count(), 96);
    assert_eq!(device::safe_label("mouse\u{0000}\u{001b}[31m"), "mouse[31m");
}

#[test]
fn reserved_feature_index_and_long_report_legacy_marker() {
    assert_eq!(
        Feature::parse(0x1000, &[0xff, 0, 0]),
        Err(Error::Malformed("reserved feature index"))
    );
    let mut report = [0; 20];
    report[..7].copy_from_slice(&[0x11, 1, 0x8f, 0x12, 3, 4, 5]);
    let packet = Packet::parse(&report).unwrap();
    assert!(matches!(
        packet.response_to(&[0x10, 1, 0x8f, 0x12]),
        Some(Ok(_))
    ));
}
