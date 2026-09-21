use logipeek::hid::{
    device::{self, Endpoint, FeatureResult},
    features::{
        battery::{self, Charging, Level},
        dpi::{self, DpiValues, SensorDpi, SetDpiError, SetDpiOutcome},
    },
    hidpp::{self, Error, Exchange, Feature, Packet, Protocol, READ_ONLY_ATTEMPTS},
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

#[test]
fn read_only_exchange_retries_timeout_then_succeeds() {
    let mut exchange = QueueExchange::new([Err(Error::Timeout), Ok(vec![1, 2, 3])]);
    assert_eq!(
        hidpp::read_only_exchange(&mut exchange, 2, 7, 1, &[9]),
        Ok(vec![1, 2, 3])
    );
    assert_eq!(exchange.calls.len(), 2);
}

#[test]
fn read_only_exchange_retries_timeout_only_once() {
    let mut exchange = QueueExchange::new([Err(Error::Timeout), Err(Error::Timeout)]);
    assert_eq!(
        hidpp::read_only_exchange(&mut exchange, 2, 7, 1, &[]),
        Err(Error::Timeout)
    );
    assert_eq!(exchange.calls.len(), READ_ONLY_ATTEMPTS as usize);
}

#[test]
fn read_only_exchange_retries_hidpp_busy_errors() {
    for error in [
        Error::Protocol {
            legacy: false,
            code: 0x08,
        },
        Error::Protocol {
            legacy: true,
            code: 0x07,
        },
    ] {
        let mut exchange = QueueExchange::new([Err(error), Ok(vec![0xaa])]);
        assert_eq!(
            hidpp::read_only_exchange(&mut exchange, 1, 2, 1, &[]),
            Ok(vec![0xaa])
        );
        assert_eq!(exchange.calls.len(), 2);
    }
}

#[test]
fn read_only_exchange_does_not_retry_non_retryable_or_malformed_errors() {
    for error in [
        Error::Io,
        Error::Protocol {
            legacy: false,
            code: 0x09,
        },
        Error::Protocol {
            legacy: true,
            code: 0x08,
        },
        Error::Malformed("bad response"),
    ] {
        let mut exchange = QueueExchange::new([Err(error.clone()), Ok(vec![1])]);
        assert_eq!(
            hidpp::read_only_exchange(&mut exchange, 1, 2, 1, &[]),
            Err(error)
        );
        assert_eq!(exchange.calls.len(), 1);
    }
}

#[test]
fn packet_response_requires_current_software_id_and_ignores_late_reply() {
    let request = [0x10, 0x02, 0x20, 0x55];
    let late = Packet {
        report: 0x10,
        device: 0x02,
        feature: 0x20,
        function: 0x54,
        parameters: vec![1],
    };
    assert_eq!(late.response_to(&request), None);

    let current = Packet {
        report: 0x10,
        device: 0x02,
        feature: 0x20,
        function: 0x55,
        parameters: vec![1],
    };
    assert_eq!(current.response_to(&request), Some(Ok(vec![1])));

    let late_error = Packet {
        report: 0x10,
        device: 0x02,
        feature: 0xff,
        function: 0x20,
        parameters: vec![0x54, 0x08],
    };
    assert_eq!(late_error.response_to(&request), None);
    let current_error = Packet {
        parameters: vec![0x55, 0x08],
        ..late_error
    };
    assert_eq!(
        current_error.response_to(&request),
        Some(Err(Error::Protocol {
            legacy: false,
            code: 0x08,
        }))
    );
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
    assert_eq!(coarse.percentage, None);
    assert_eq!(coarse.level, Some(Level::Good));
    assert_eq!(coarse.charging, Charging::Discharging);
    let percentage = battery::parse_1000(&[10, 2], &[80, 20, 1]).unwrap();
    assert_eq!(percentage.percentage, Some(80));
    assert_eq!(percentage.level, None);
    assert_eq!(percentage.charging, Charging::Charging);
}

#[test]
fn battery_zero_is_unknown_and_invalid_ranges_are_rejected() {
    assert_eq!(
        battery::parse_1000(&[10, 2], &[0, 0, 3])
            .unwrap()
            .percentage,
        None
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
fn unified_battery_parses_percentage_coarse_and_unknown_states() {
    let percentage = battery::parse_1004(3, &[0x0f, 0x03], &[74, 0x04, 1, 1]).unwrap();
    assert_eq!(percentage.feature_id, 0x1004);
    assert_eq!(percentage.feature_version, 3);
    assert_eq!(percentage.percentage, Some(74));
    assert_eq!(percentage.level, Some(Level::Good));
    assert_eq!(percentage.charging, Charging::Charging);
    assert_eq!(percentage.rechargeable, Some(true));
    assert_eq!(percentage.external_power_raw, Some(1));

    let coarse = battery::parse_1004(2, &[0x0f, 0x01], &[0xff, 0x02, 0, 0]).unwrap();
    assert_eq!(coarse.percentage, None);
    assert_eq!(coarse.level, Some(Level::Low));
    assert_eq!(coarse.charging, Charging::Discharging);

    let unknown = battery::parse_1004(3, &[0x0f, 0x03], &[40, 0x10, 0xa5, 0xfe]).unwrap();
    assert_eq!(unknown.level, Some(Level::Unknown(0x10)));
    assert_eq!(unknown.charging, Charging::Unknown(0xa5));
    assert_eq!(unknown.external_power_raw, Some(0xfe));
}

#[test]
fn unified_battery_rejects_truncation_and_invalid_percentage() {
    for (capabilities, status) in [
        (&[][..], &[74, 4, 1, 1][..]),
        (&[0x0f][..], &[74, 4, 1, 1][..]),
        (&[0x0f, 3][..], &[][..]),
        (&[0x0f, 3][..], &[74, 4, 1][..]),
    ] {
        assert_eq!(
            battery::parse_1004(3, capabilities, status),
            Err(Error::Malformed("truncated unified battery data"))
        );
    }
    assert_eq!(
        battery::parse_1004(3, &[0x0f, 3], &[101, 4, 0, 0]),
        Err(Error::Malformed("invalid battery percentage"))
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
    assert_eq!(battery.percentage, Some(74));
    assert_eq!(battery.charging, Charging::Charging);
    assert_eq!(exchange.calls, vec![(3, 7, 1, vec![]), (3, 7, 0, vec![])]);
}

#[test]
fn unified_battery_read_uses_dynamic_index_and_propagates_errors() {
    let feature = Feature {
        id: 0x1004,
        index: 6,
        flags: 0,
        version: 3,
    };
    let mut exchange = QueueExchange::new([Ok(vec![0x0f, 3]), Ok(vec![74, 4, 1, 1])]);
    let result = battery::read(&mut exchange, 1, feature).unwrap();
    assert_eq!(result.percentage, Some(74));
    assert_eq!(result.feature_version, 3);
    assert_eq!(exchange.calls, vec![(1, 6, 0, vec![]), (1, 6, 1, vec![])]);

    let mut capability_error = QueueExchange::new([Err(Error::Io)]);
    assert_eq!(
        battery::read(&mut capability_error, 1, feature),
        Err(Error::Read {
            operation: "0x1004 capabilities (fn0)",
            source: Box::new(Error::Io),
        })
    );
    assert_eq!(capability_error.calls, vec![(1, 6, 0, vec![])]);

    let mut status_error = QueueExchange::new([Ok(vec![0x0f, 3]), Err(Error::Timeout)]);
    assert_eq!(
        battery::read(&mut status_error, 1, feature),
        Err(Error::Read {
            operation: "0x1004 status (fn1)",
            source: Box::new(Error::Timeout),
        })
    );
    assert_eq!(
        status_error.calls,
        vec![(1, 6, 0, vec![]), (1, 6, 1, vec![]), (1, 6, 1, vec![])]
    );
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
    assert_eq!(
        battery::read(&mut exchange, 3, feature),
        Err(Error::Read {
            operation: "0x1000 capabilities (fn1)",
            source: Box::new(Error::Io),
        })
    );
    assert_eq!(exchange.calls, vec![(3, 7, 1, vec![])]);
}

#[test]
fn dpi_parses_discrete_values_and_ranges() {
    assert_eq!(
        dpi::parse_dpi_values(0, &[0, 0x01, 0x90, 0x03, 0x20, 0x06, 0x40, 0, 0]),
        Ok(DpiValues::List(vec![400, 800, 1600]))
    );
    assert_eq!(
        dpi::parse_dpi_values(0, &[0, 0x01, 0x90, 0xe0, 0x32, 0x0c, 0x80, 0, 0]),
        Ok(DpiValues::Range {
            minimum: 400,
            maximum: 3200,
            step: 50,
        })
    );
    assert_eq!(
        dpi::parse_dpi_values(0, &[0, 0x01, 0x90, 0xe0, 0x3c, 0x0c, 0x80, 0, 0]),
        Ok(DpiValues::Range {
            minimum: 400,
            maximum: 3200,
            step: 60,
        })
    );
    assert_eq!(
        dpi::parse_dpi_values(
            0,
            &[0, 0, 100, 0, 200, 1, 44, 1, 144, 1, 244, 2, 88, 2, 188, 0],
        ),
        Ok(DpiValues::List(vec![100, 200, 300, 400, 500, 600, 700]))
    );
}

#[test]
fn dpi_rejects_bad_sentinels_ranges_and_terminators() {
    for (data, expected) in [
        (
            &[0, 0xe0, 0x32, 0x03, 0x20, 0, 0][..],
            Error::Malformed("unexpected DPI range sentinel"),
        ),
        (
            &[0, 0x01, 0x90, 0xe0, 0x00, 0x0c, 0x80, 0, 0][..],
            Error::Malformed("invalid DPI range"),
        ),
        (
            &[0, 0x01, 0x90, 0xe0, 0x32, 0, 0][..],
            Error::Malformed("unexpected DPI range sentinel"),
        ),
        (
            &[0, 0x01, 0x90, 0xe0, 0x32, 0xe0, 0x64, 0, 0][..],
            Error::Malformed("unexpected DPI range sentinel"),
        ),
        (
            &[0, 0x0c, 0x80, 0xe0, 0x32, 0x01, 0x90, 0, 0][..],
            Error::Malformed("invalid DPI range"),
        ),
        (
            &[0, 0x01, 0x90, 0, 0, 1][..],
            Error::Malformed("nonzero data after DPI terminator"),
        ),
    ] {
        assert_eq!(dpi::parse_dpi_values(0, data), Err(expected));
    }
}

#[test]
fn dpi_support_and_nearest_cover_lists_ranges_ties_and_bounds() {
    let list = DpiValues::List(vec![400, 800, 1600]);
    assert!(dpi::supports_dpi(&list, 800));
    assert!(!dpi::supports_dpi(&list, 801));
    assert_eq!(dpi::nearest_supported_dpi(&list, 600), Some(800));
    assert_eq!(dpi::nearest_supported_dpi(&list, 100), Some(400));
    assert_eq!(dpi::nearest_supported_dpi(&list, 2000), Some(1600));
    let range = DpiValues::Range {
        minimum: 400,
        maximum: 1600,
        step: 100,
    };
    assert!(dpi::supports_dpi(&range, 400));
    assert!(dpi::supports_dpi(&range, 900));
    assert!(dpi::supports_dpi(&range, 1600));
    assert!(!dpi::supports_dpi(&range, 950));
    assert_eq!(dpi::nearest_supported_dpi(&range, 350), Some(400));
    assert_eq!(dpi::nearest_supported_dpi(&range, 450), Some(500));
    assert_eq!(dpi::nearest_supported_dpi(&range, 1700), Some(1600));
    let invalid_range = DpiValues::Range {
        minimum: 400,
        maximum: 1600,
        step: 0,
    };
    assert!(!dpi::supports_dpi(&invalid_range, 400));
    assert_eq!(dpi::nearest_supported_dpi(&invalid_range, 800), None);
}

fn writable_sensor() -> SensorDpi {
    SensorDpi {
        sensor: 2,
        current: 800,
        default: None,
        supported: DpiValues::List(vec![400, 800, 1600]),
    }
}

#[test]
fn dpi_set_validates_before_exchange_and_writes_sensor_and_value_big_endian_once() {
    let sensor = writable_sensor();
    let feature = Feature {
        id: 0x2201,
        index: 9,
        flags: 0,
        version: 3,
    };
    for (feature, sensor_count, requested, expected) in [
        (
            Feature {
                id: 0x2202,
                ..feature
            },
            3,
            800,
            SetDpiError::InvalidFeature,
        ),
        (feature, 2, 800, SetDpiError::InvalidSensor),
        (
            feature,
            3,
            801,
            SetDpiError::Unsupported {
                requested: 801,
                nearest: Some(800),
            },
        ),
    ] {
        let mut exchange = QueueExchange::new(Vec::<Result<Vec<u8>, Error>>::new());
        assert_eq!(
            dpi::set_and_verify(&mut exchange, 1, feature, sensor_count, &sensor, requested),
            Err(expected)
        );
        assert!(exchange.calls.is_empty());
    }
    let mut exchange =
        QueueExchange::new([Ok(vec![2, 0x06, 0x40]), Ok(vec![2, 0x06, 0x40, 0x00, 0x00])]);
    assert_eq!(
        dpi::set_and_verify(&mut exchange, 1, feature, 3, &sensor, 1600),
        Ok(SetDpiOutcome::Verified { current: 1600 })
    );
    assert_eq!(
        exchange.calls,
        vec![(1, 9, 3, vec![2, 0x06, 0x40]), (1, 9, 2, vec![2])]
    );
}

#[test]
fn dpi_set_handles_ack_mismatch_and_readback_outcomes_without_retrying_fn3() {
    let sensor = writable_sensor();
    let feature = Feature {
        id: 0x2201,
        index: 9,
        flags: 0,
        version: 3,
    };
    let mut bad_ack = QueueExchange::new([Ok(vec![2, 0x03, 0x21])]);
    assert_eq!(
        dpi::set_and_verify(&mut bad_ack, 1, feature, 3, &sensor, 1600),
        Err(SetDpiError::Write(Error::Malformed(
            "DPI setter acknowledgement mismatch"
        )))
    );
    assert_eq!(bad_ack.calls.len(), 1);
    let protocol_error = Error::Protocol {
        legacy: false,
        code: 0x08,
    };
    let mut rejected = QueueExchange::new([Err(protocol_error.clone()), Ok(vec![2, 0x06, 0x40])]);
    assert_eq!(
        dpi::set_and_verify(&mut rejected, 1, feature, 3, &sensor, 1600),
        Err(SetDpiError::Write(protocol_error))
    );
    assert_eq!(rejected.calls, vec![(1, 9, 3, vec![2, 0x06, 0x40])]);
    for (replies, expected) in [
        (
            vec![Ok(vec![2, 0x06, 0x40]), Ok(vec![2, 0x03, 0x20, 0x03, 0x20])],
            SetDpiOutcome::AcknowledgedMismatch { actual: 800 },
        ),
        (
            vec![
                Ok(vec![2, 0x06, 0x40]),
                Err(Error::Timeout),
                Err(Error::Timeout),
            ],
            SetDpiOutcome::AcknowledgedUnverified {
                error: Error::Read {
                    operation: "0x2201 DPI write read-back (fn2)",
                    source: Box::new(Error::Timeout),
                },
            },
        ),
    ] {
        let mut exchange = QueueExchange::new(replies);
        assert_eq!(
            dpi::set_and_verify(&mut exchange, 1, feature, 3, &sensor, 1600),
            Ok(expected)
        );
        assert_eq!(exchange.calls.iter().filter(|call| call.2 == 3).count(), 1);
    }
}

#[test]
fn dpi_set_timeout_uses_readback_to_classify_confirmed_different_or_unverified() {
    let sensor = writable_sensor();
    let feature = Feature {
        id: 0x2201,
        index: 9,
        flags: 0,
        version: 3,
    };
    for (replies, expected) in [
        (
            vec![Err(Error::Timeout), Ok(vec![2, 0x06, 0x40, 0, 0])],
            SetDpiOutcome::TimedOutConfirmed { current: 1600 },
        ),
        (
            vec![Err(Error::Timeout), Ok(vec![2, 0x03, 0x20, 0, 0])],
            SetDpiOutcome::TimedOutDifferent { actual: 800 },
        ),
        (
            vec![
                Err(Error::Timeout),
                Err(Error::Timeout),
                Err(Error::Timeout),
            ],
            SetDpiOutcome::TimedOutUnverified {
                error: Error::Read {
                    operation: "0x2201 DPI write read-back (fn2)",
                    source: Box::new(Error::Timeout),
                },
            },
        ),
    ] {
        let mut exchange = QueueExchange::new(replies);
        assert_eq!(
            dpi::set_and_verify(&mut exchange, 1, feature, 3, &sensor, 1600),
            Ok(expected)
        );
        assert_eq!(exchange.calls.iter().filter(|call| call.2 == 3).count(), 1);
    }
}

#[test]
fn dpi_parsers_reject_truncation_bad_indices_and_values() {
    assert_eq!(
        dpi::parse_sensor_count(&[]),
        Err(Error::Malformed("truncated DPI sensor count"))
    );
    for count in [0, 17] {
        assert_eq!(
            dpi::parse_sensor_count(&[count]),
            Err(Error::Malformed("invalid DPI sensor count"))
        );
    }
    assert_eq!(dpi::parse_sensor_count(&[2]), Ok(2));
    assert_eq!(
        dpi::parse_dpi_values(0, &[0, 1]),
        Err(Error::Malformed("truncated DPI list response"))
    );
    assert_eq!(
        dpi::parse_dpi_values(0, &[1, 0x01, 0x90, 0, 0]),
        Err(Error::Malformed("DPI sensor index mismatch"))
    );

    let supported = DpiValues::List(vec![400, 800, 1600]);
    assert_eq!(
        dpi::parse_sensor_dpi(0, supported.clone(), &[0, 1, 0x90, 3]),
        Err(Error::Malformed("truncated current DPI response"))
    );
    assert_eq!(
        dpi::parse_sensor_dpi(0, supported.clone(), &[1, 1, 0x90, 3, 0x20]),
        Err(Error::Malformed("DPI sensor index mismatch"))
    );
    for response in [[0, 0, 0, 3, 0x20], [0, 0xe0, 0, 3, 0x20]] {
        assert_eq!(
            dpi::parse_sensor_dpi(0, supported.clone(), &response),
            Err(Error::Malformed("invalid current DPI"))
        );
    }
    assert_eq!(
        dpi::parse_sensor_dpi(0, supported.clone(), &[0, 1, 0x90, 0xe0, 0]),
        Err(Error::Malformed("invalid default DPI"))
    );
    assert_eq!(
        dpi::parse_sensor_dpi(0, supported.clone(), &[0, 1, 0x90, 3, 0x20]),
        Ok(SensorDpi {
            sensor: 0,
            current: 400,
            default: Some(800),
            supported,
        })
    );
}

#[test]
fn dpi_read_handles_multiple_sensors_and_exact_call_order() {
    let feature = Feature {
        id: 0x2201,
        index: 10,
        flags: 0,
        version: 2,
    };
    let mut exchange = QueueExchange::new([
        Ok(vec![2]),
        Ok(vec![0, 0x01, 0x90, 0x03, 0x20, 0, 0]),
        Ok(vec![0, 0x03, 0x20, 0x01, 0x90]),
        Ok(vec![1, 0x01, 0x2c, 0xe0, 0x32, 0x06, 0x40, 0, 0]),
        Ok(vec![1, 0x04, 0xb0, 0, 0]),
    ]);
    assert_eq!(
        dpi::read(&mut exchange, 1, feature),
        Ok(vec![
            SensorDpi {
                sensor: 0,
                current: 800,
                default: Some(400),
                supported: DpiValues::List(vec![400, 800]),
            },
            SensorDpi {
                sensor: 1,
                current: 1200,
                default: None,
                supported: DpiValues::Range {
                    minimum: 300,
                    maximum: 1600,
                    step: 50,
                },
            },
        ])
    );
    assert_eq!(
        exchange.calls,
        vec![
            (1, 10, 0, vec![]),
            (1, 10, 1, vec![0]),
            (1, 10, 2, vec![0]),
            (1, 10, 1, vec![1]),
            (1, 10, 2, vec![1]),
        ]
    );
}

#[test]
fn dpi_read_propagates_count_and_midstream_errors() {
    let feature = Feature {
        id: 0x2201,
        index: 10,
        flags: 0,
        version: 2,
    };
    let mut count_error = QueueExchange::new([Err(Error::Io)]);
    assert_eq!(
        dpi::read(&mut count_error, 1, feature),
        Err(Error::Read {
            operation: "0x2201 sensor count (fn0)",
            source: Box::new(Error::Io),
        })
    );
    assert_eq!(count_error.calls, vec![(1, 10, 0, vec![])]);

    let mut midstream_error = QueueExchange::new([
        Ok(vec![2]),
        Ok(vec![0, 0x01, 0x90, 0, 0]),
        Ok(vec![0, 0x01, 0x90, 0, 0]),
        Err(Error::Timeout),
    ]);
    assert_eq!(
        dpi::read(&mut midstream_error, 1, feature),
        Err(Error::Read {
            operation: "0x2201 supported DPI (fn1)",
            source: Box::new(Error::Timeout),
        })
    );
    assert_eq!(
        midstream_error.calls,
        vec![
            (1, 10, 0, vec![]),
            (1, 10, 1, vec![0]),
            (1, 10, 2, vec![0]),
            (1, 10, 1, vec![1]),
            (1, 10, 1, vec![1]),
        ]
    );

    let mut unsupported = QueueExchange::new(Vec::<Result<Vec<u8>, Error>>::new());
    let extended_feature = Feature {
        id: 0x2202,
        ..feature
    };
    assert_eq!(
        dpi::read(&mut unsupported, 1, extended_feature),
        Err(Error::Malformed("DPI feature not implemented"))
    );
    assert!(unsupported.calls.is_empty());
}

fn endpoint_with_features(features: Vec<FeatureResult>) -> Endpoint {
    Endpoint {
        index: 1,
        opaque_id: "test-device".into(),
        protocol: Ok(Protocol::Feature { major: 2, minor: 0 }),
        features,
        battery: None,
        dpi: None,
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
