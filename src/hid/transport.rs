use super::hidpp::{Error, Exchange, Packet};
use hidapi::HidDevice;
use std::time::{Duration, Instant};

/// One physical request gets one bounded response window. Retries belong only
/// in the explicit read-only layer.
pub const RESPONSE_TIMEOUT: Duration = Duration::from_millis(750);

pub struct Transport {
    handle: HidDevice,
    report: u8,
    software_id: u8,
}
impl Transport {
    pub fn new(handle: HidDevice, long_reports: bool) -> Self {
        Self {
            handle,
            report: if long_reports { 0x11 } else { 0x10 },
            software_id: 0,
        }
    }
}
impl Exchange for Transport {
    fn exchange(
        &mut self,
        device: u8,
        feature: u8,
        function: u8,
        parameters: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let size = if self.report == 0x11 { 20 } else { 7 };
        if parameters.len() > size - 4 || function > 15 {
            return Err(Error::Malformed("invalid request"));
        }
        self.software_id = self.software_id % 15 + 1;
        let mut request = vec![0; size];
        request[..4].copy_from_slice(&[
            self.report,
            device,
            feature,
            (function << 4) | self.software_id,
        ]);
        request[4..4 + parameters.len()].copy_from_slice(parameters);
        // windows-native may return 0 for synchronous success, or the padded report size.
        let written = self.handle.write(&request).map_err(|_| Error::Io)?;
        if written != 0 && written < request.len() {
            return Err(Error::Io);
        }
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        let mut buffer = [0u8; 64];
        // Bound both elapsed time and unrelated traffic; never poll in the background.
        for _ in 0..128 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let timeout = remaining.as_millis().clamp(1, RESPONSE_TIMEOUT.as_millis()) as i32;
            let count = self
                .handle
                .read_timeout(&mut buffer, timeout)
                .map_err(|_| Error::Io)?;
            if count == 0 {
                break;
            }
            // Non-HID++ traffic is not a response. Malformed matching reports are errors.
            if !matches!(buffer[0], 0x10 | 0x11) {
                continue;
            }
            match parse_input(&buffer[..count]) {
                Ok(packet) => {
                    if let Some(result) = packet.response_to(&request) {
                        return result;
                    }
                }
                Err(error) => {
                    if count >= 4 && buffer[1..4] == request[1..4] {
                        return Err(error);
                    }
                }
            }
        }
        Err(Error::Timeout)
    }
}

/// Windows can pad shorter reports to the collection's maximum input size.
/// Only zero padding is accepted; the protocol parser itself remains strict.
pub fn parse_input(bytes: &[u8]) -> Result<Packet, Error> {
    let size = match bytes.first() {
        Some(0x10) => 7,
        Some(0x11) => 20,
        _ => return Err(Error::Malformed("unsupported report ID")),
    };
    if bytes.len() < size || bytes.len() > 64 || bytes[size..].iter().any(|byte| *byte != 0) {
        return Err(Error::Malformed("invalid HID report padding or length"));
    }
    Packet::parse(&bytes[..size])
}
