use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Io,
    Timeout,
    Malformed(&'static str),
    Protocol {
        legacy: bool,
        code: u8,
    },
    Read {
        operation: &'static str,
        source: Box<Error>,
    },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io => f.write_str("HID I/O unavailable (access denied, disconnected, or busy)"),
            Self::Timeout => {
                f.write_str("No matching reply before timeout (sleeping or unavailable)")
            }
            Self::Malformed(reason) => write!(f, "Malformed response: {reason}"),
            Self::Protocol { legacy, code } => write!(
                f,
                "HID++ {} error 0x{code:02X}",
                if *legacy { "1.x" } else { "2.x" }
            ),
            Self::Read { operation, source } => write!(f, "{operation}: {source}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Error {
    pub fn in_read(self, operation: &'static str) -> Self {
        Self::Read {
            operation,
            source: Box::new(self),
        }
    }

    pub fn is_retryable_read(&self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::Protocol {
                    legacy: true,
                    code: 0x07
                }
                | Self::Protocol {
                    legacy: false,
                    code: 0x08
                }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub report: u8,
    pub device: u8,
    pub feature: u8,
    pub function: u8,
    pub parameters: Vec<u8>,
}
impl Packet {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let length = match bytes.first() {
            Some(0x10) => 7,
            Some(0x11) => 20,
            _ => return Err(Error::Malformed("unsupported report ID")),
        };
        if bytes.len() != length {
            return Err(Error::Malformed("incorrect report length"));
        }
        Ok(Self {
            report: bytes[0],
            device: bytes[1],
            feature: bytes[2],
            function: bytes[3],
            parameters: bytes[4..].to_vec(),
        })
    }

    /// Ignores other devices, notifications, and other clients' replies.
    pub fn response_to(&self, request: &[u8]) -> Option<Result<Vec<u8>, Error>> {
        if request.len() < 4 || self.device != request[1] {
            return None;
        }
        if (self.report == 0x10 && self.feature == 0x8f) || self.feature == 0xff {
            if self.function != request[2] || self.parameters.first().copied() != Some(request[3]) {
                return None;
            }
            return Some(match self.parameters.get(1) {
                Some(code) => Err(Error::Protocol {
                    legacy: self.feature == 0x8f,
                    code: *code,
                }),
                None => Err(Error::Malformed("truncated protocol error")),
            });
        }
        if self.feature == request[2] && self.function == request[3] {
            Some(Ok(self.parameters.clone()))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub id: u16,
    pub index: u8,
    pub flags: u8,
    pub version: u8,
}
impl Feature {
    pub fn parse(id: u16, data: &[u8]) -> Result<Option<Self>, Error> {
        if data.len() < 3 {
            return Err(Error::Malformed("truncated feature response"));
        }
        if data[0] == 0xff {
            return Err(Error::Malformed("reserved feature index"));
        }
        if data[0] == 0 && id != 0 {
            return Ok(None);
        }
        Ok(Some(Self {
            id,
            index: data[0],
            flags: data[1],
            version: data[2],
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Legacy,
    Feature { major: u8, minor: u8 },
}

pub trait Exchange {
    fn exchange(
        &mut self,
        device: u8,
        feature: u8,
        function: u8,
        parameters: &[u8],
    ) -> Result<Vec<u8>, Error>;
}

pub const READ_ONLY_ATTEMPTS: u8 = 2;

/// Retry only an idempotent, read-only request. Set/write/reset operations must
/// call `Exchange::exchange` directly so a timeout can never duplicate a write.
pub fn read_only_exchange(
    transport: &mut impl Exchange,
    device: u8,
    feature: u8,
    function: u8,
    parameters: &[u8],
) -> Result<Vec<u8>, Error> {
    let mut attempt = 1;
    loop {
        match transport.exchange(device, feature, function, parameters) {
            Err(error) if attempt < READ_ONLY_ATTEMPTS && error.is_retryable_read() => {
                attempt += 1;
            }
            result => return result,
        }
    }
}

pub fn probe(transport: &mut impl Exchange, device: u8) -> Result<Protocol, Error> {
    let ping = 0xa5;
    match read_only_exchange(transport, device, 0, 1, &[0, 0, ping]) {
        Err(Error::Protocol {
            legacy: true,
            code: 1,
        }) => Ok(Protocol::Legacy),
        Ok(data) if data.len() >= 3 && data[2] == ping && data[0] >= 2 => Ok(Protocol::Feature {
            major: data[0],
            minor: data[1],
        }),
        Ok(_) => Err(Error::Malformed("invalid protocol version or ping echo")),
        Err(e) => Err(e),
    }
}

pub fn discover(
    transport: &mut impl Exchange,
    device: u8,
    id: u16,
) -> Result<Option<Feature>, Error> {
    Feature::parse(
        id,
        &read_only_exchange(transport, device, 0, 0, &id.to_be_bytes())?,
    )
}
