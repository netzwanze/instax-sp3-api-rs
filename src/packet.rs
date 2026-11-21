//! Instax SP-2 / SP-3 wire protocol (std only, no dependencies).
//!
//! Command packet (client -> printer), all integers big-endian:
//!
//! ```text
//! 0      1      2..4   4..8      8..10  10..12  12..          len-4..len-2  len-2..len
//! [0x24] [type] [len]  [session] [pin]  [0 0]   [payload...]  [checksum]    [0x0D 0x0A]
//! ```
//! `len` = 16 + payload length. `checksum` = bitwise NOT of the 16-bit
//! truncated sum of every byte before it.
//!
//! Response packet (printer -> client):
//!
//! ```text
//! 0      1      2..4   4..8      8..12   12      13   14        15                16..
//! [0x2A] [type] [len]  [session] [0 x4]  [retcode] [?] [eject<<2] [batt<<4|prints] [payload...]
//! ```

use std::fmt;

pub const MODE_COMMAND: u8 = 36;
pub const MODE_RESPONSE: u8 = 42;

const T_SPECIFICATIONS: u8 = 79;
const T_RESET: u8 = 80;
const T_PREP_IMAGE: u8 = 81;
const T_SEND_IMAGE: u8 = 82;
const T_83: u8 = 83;
const T_SET_LOCK_STATE: u8 = 176;
const T_LOCK_DEVICE: u8 = 179;
const T_VERSION: u8 = 192;
const T_PRINT_COUNT: u8 = 193;
const T_MODEL_NAME: u8 = 194;
const T_195: u8 = 195;
const T_PRE_PRINT: u8 = 196;

/// Return code meaning "OK" / "frame received" (`RTN_E_RCV_FRAME` upstream).
pub const RTN_OK: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketError {
    TooShort(usize),
    BadMode(u8),
    LengthMismatch { declared: usize, actual: usize },
    BadChecksum,
    BadTrailer,
    PayloadTooLong(usize),
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "packet too short ({n} bytes)"),
            Self::BadMode(m) => write!(f, "unexpected start byte {m} (expected {MODE_RESPONSE})"),
            Self::LengthMismatch { declared, actual } => {
                write!(f, "length field says {declared} bytes but got {actual}")
            }
            Self::BadChecksum => write!(f, "checksum mismatch"),
            Self::BadTrailer => write!(f, "missing CR LF trailer"),
            Self::PayloadTooLong(n) => write!(f, "payload of {n} bytes does not fit in a packet"),
        }
    }
}

impl std::error::Error for PacketError {}

/// Every command the Python library can send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command<'a> {
    Specifications,
    Version,
    PrintCount,
    ModelName,
    PrePrint(u16),
    Lock(u8),
    Reset,
    PrepImage { format: u8, options: u8, img_len: u32 },
    SendImage { seq: u32, data: &'a [u8] },
    Type83,
    Type195,
    LockState,
}

impl Command<'_> {
    pub fn type_byte(&self) -> u8 {
        match self {
            Self::Specifications => T_SPECIFICATIONS,
            Self::Version => T_VERSION,
            Self::PrintCount => T_PRINT_COUNT,
            Self::ModelName => T_MODEL_NAME,
            Self::PrePrint(_) => T_PRE_PRINT,
            Self::Lock(_) => T_LOCK_DEVICE,
            Self::Reset => T_RESET,
            Self::PrepImage { .. } => T_PREP_IMAGE,
            Self::SendImage { .. } => T_SEND_IMAGE,
            Self::Type83 => T_83,
            Self::Type195 => T_195,
            Self::LockState => T_SET_LOCK_STATE,
        }
    }

    fn payload(&self) -> Vec<u8> {
        match self {
            Self::PrePrint(n) => {
                let mut p = vec![0, 0];
                p.extend_from_slice(&n.to_be_bytes());
                p
            }
            Self::Lock(state) => vec![*state, 0, 0, 0],
            Self::PrepImage { format, options, img_len } => {
                let mut p = vec![*format, *options];
                p.extend_from_slice(&img_len.to_be_bytes());
                p.extend_from_slice(&[0; 6]);
                p
            }
            Self::SendImage { seq, data } => {
                let mut p = Vec::with_capacity(4 + data.len());
                p.extend_from_slice(&seq.to_be_bytes());
                p.extend_from_slice(data);
                p
            }
            _ => Vec::new(),
        }
    }
}

/// Sum of bytes, truncated to 16 bits, bitwise-NOT'd.
fn checksum(bytes: &[u8]) -> u16 {
    let sum: u32 = bytes.iter().map(|&b| b as u32).sum();
    !(sum as u16)
}

pub fn encode_command(cmd: &Command, session: u32, pin: u16) -> Result<Vec<u8>, PacketError> {
    let payload = cmd.payload();
    let total = 16 + payload.len();
    if total > u16::MAX as usize {
        return Err(PacketError::PayloadTooLong(payload.len()));
    }
    let mut v = Vec::with_capacity(total);
    v.push(MODE_COMMAND);
    v.push(cmd.type_byte());
    v.extend_from_slice(&(total as u16).to_be_bytes());
    v.extend_from_slice(&session.to_be_bytes());
    v.extend_from_slice(&pin.to_be_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(&payload);
    let ck = checksum(&v);
    v.extend_from_slice(&ck.to_be_bytes());
    v.extend_from_slice(&[13, 10]);
    Ok(v)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub unknown1: u16,
    pub firmware: String,
    pub hardware: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specifications {
    pub max_width: u16,
    pub max_height: u16,
    pub max_colours: u16,
    pub unknown1: u16,
    pub max_msg_size: u16,
    pub unknown2: u8,
    pub unknown3: u32,
}

/// A validated response packet.
#[derive(Debug, Clone)]
pub struct Response {
    pub type_byte: u8,
    pub session: u32,
    pub return_code: u8,
    pub ejecting: u8,
    /// Battery level, 0..=7 (raw; the meaning of the scale is not documented upstream).
    pub battery: u8,
    /// "Prints left" nibble from the header (0..=15).
    pub prints_left: u8,
    raw: Vec<u8>,
}

impl Response {
    pub fn parse(bytes: &[u8]) -> Result<Self, PacketError> {
        if bytes.len() < 20 {
            return Err(PacketError::TooShort(bytes.len()));
        }
        if bytes[0] != MODE_RESPONSE {
            return Err(PacketError::BadMode(bytes[0]));
        }
        let declared = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
        if declared != bytes.len() {
            return Err(PacketError::LengthMismatch { declared, actual: bytes.len() });
        }
        let n = bytes.len();
        if bytes[n - 2] != 13 || bytes[n - 1] != 10 {
            return Err(PacketError::BadTrailer);
        }
        let sum: u32 = bytes[..n - 4].iter().map(|&b| b as u32).sum();
        let stored = u16::from_be_bytes([bytes[n - 4], bytes[n - 3]]) as u32;
        if (sum + stored) & 0xFFFF != 0xFFFF {
            return Err(PacketError::BadChecksum);
        }
        Ok(Self {
            type_byte: bytes[1],
            session: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            return_code: bytes[12],
            ejecting: bytes[14] >> 2,
            battery: (bytes[15] >> 4) & 7,
            prints_left: bytes[15] & 15,
            raw: bytes.to_vec(),
        })
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    fn u8_at(&self, o: usize) -> u8 {
        self.raw.get(o).copied().unwrap_or(0)
    }
    fn u16_at(&self, o: usize) -> u16 {
        match self.raw.get(o..o + 2) {
            Some(b) => u16::from_be_bytes([b[0], b[1]]),
            None => 0,
        }
    }
    fn u32_at(&self, o: usize) -> u32 {
        match self.raw.get(o..o + 4) {
            Some(b) => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            None => 0,
        }
    }

    pub fn version(&self) -> Version {
        let dot = |v: u16| format!("{:02X}.{:02X}", v >> 8, v & 0xFF);
        Version {
            unknown1: self.u16_at(16),
            firmware: dot(self.u16_at(18)),
            hardware: dot(self.u16_at(20)),
        }
    }

    pub fn model_name(&self) -> String {
        match self.raw.get(16..20) {
            Some(b) => String::from_utf8_lossy(b).into_owned(),
            None => String::new(),
        }
    }

    pub fn print_count(&self) -> u32 {
        self.u32_at(16)
    }

    pub fn specifications(&self) -> Specifications {
        Specifications {
            max_width: self.u16_at(16),
            max_height: self.u16_at(18),
            max_colours: self.u16_at(20),
            unknown1: self.u16_at(22),
            max_msg_size: self.u16_at(28),
            unknown2: self.u8_at(30),
            unknown3: self.u32_at(32),
        }
    }

    /// Names for the return codes upstream defines as faults.
    /// (`RTN_E_*` constants in the Python `Packet` class.)
    pub fn error(&self) -> Option<&'static str> {
        match self.return_code {
            248 => Some("paper-insertion sensor error"),
            247 => Some("wrong PIN"),
            246 => Some("motor error"),
            245 => Some("camera point error"),
            244 => Some("film empty"),
            240..=243 => Some("frame receive error"),
            162 => Some("battery empty"),
            161 => Some("printer rejected the image data"),
            160 => Some("printer is in use by another client"),
            _ => None,
        }
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors generated by the original Python implementation
    // (session = 0x63fb4a6b, pin = 1111).
    const SESSION: u32 = 0x63fb4a6b;
    const PIN: u16 = 1111;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn check(cmd: Command, expected: &str) {
        let got = encode_command(&cmd, SESSION, PIN).unwrap();
        assert_eq!(to_hex(&got), expected, "{cmd:?}");
    }

    #[test]
    fn encodes_every_command_like_python() {
        check(Command::Specifications, "244f001063fb4a6b04570000fd0e0d0a");
        check(Command::Version, "24c0001063fb4a6b04570000fc9d0d0a");
        check(Command::PrintCount, "24c1001063fb4a6b04570000fc9c0d0a");
        check(Command::ModelName, "24c2001063fb4a6b04570000fc9b0d0a");
        check(Command::PrePrint(1), "24c4001463fb4a6b0457000000000001fc940d0a");
        check(Command::PrePrint(8), "24c4001463fb4a6b0457000000000008fc8d0d0a");
        check(Command::Lock(1), "24b3001463fb4a6b0457000001000000fca50d0a");
        check(Command::Reset, "2450001063fb4a6b04570000fd0d0d0a");
        check(
            Command::PrepImage { format: 16, options: 0, img_len: 1_920_000 },
            "2451001c63fb4a6b045700001000001d4c00000000000000fc870d0a",
        );
        check(
            Command::PrepImage { format: 16, options: 0, img_len: 1_440_000 },
            "2451001c63fb4a6b0457000010000015f900000000000000fbe20d0a",
        );
        check(
            Command::SendImage { seq: 3, data: &[1, 2, 3, 4, 5] },
            "2452001963fb4a6b04570000000000030102030405fcf00d0a",
        );
        check(Command::Type83, "2453001063fb4a6b04570000fd0a0d0a");
        check(Command::Type195, "24c3001063fb4a6b04570000fc9a0d0a");
        check(Command::LockState, "24b0001063fb4a6b04570000fcad0d0a");
    }

    #[test]
    fn full_size_image_chunk_fits() {
        let data = vec![0u8; 60_000];
        let p = encode_command(&Command::SendImage { seq: 31, data: &data }, SESSION, PIN).unwrap();
        assert_eq!(p.len(), 60_020);
        assert_eq!(u16::from_be_bytes([p[2], p[3]]) as usize, 60_020);
    }

    #[test]
    fn decodes_version() {
        let r = Response::parse(&hex("2ac0001c63fb4a6b0000000000000034000101020a0b0000fc990d0a")).unwrap();
        assert_eq!(r.session, SESSION);
        assert_eq!(r.return_code, RTN_OK);
        assert_eq!(r.battery, 3);
        assert_eq!(r.prints_left, 4);
        let v = r.version();
        assert_eq!(v.unknown1, 1);
        assert_eq!(v.firmware, "01.02");
        assert_eq!(v.hardware, "0A.0B");
    }

    #[test]
    fn decodes_model_and_count() {
        let m = Response::parse(&hex("2ac2001863fb4a6b000000000000003453502d33fbb10d0a")).unwrap();
        assert_eq!(m.model_name(), "SP-3");
        let c = Response::parse(&hex(
            "2ac1002463fb4a6b000000000000003400000014000000000000000000000000fc950d0a",
        ))
        .unwrap();
        assert_eq!(c.print_count(), 20);
    }

    #[test]
    fn decodes_status_and_errors() {
        let ok = Response::parse(&hex("2ac3001463fb4a6b0000000000000021fcca0d0a")).unwrap();
        assert_eq!((ok.battery, ok.prints_left, ok.return_code), (2, 1, 0));
        assert_eq!(ok.error(), None);
        let bad = Response::parse(&hex("2ac3001463fb4a6b00000000f4000021fbd60d0a")).unwrap();
        assert_eq!(bad.return_code, 244);
        assert_eq!(bad.error(), Some("film empty"));
    }

    #[test]
    fn rejects_corrupt_packets() {
        let mut good = hex("2ac3001463fb4a6b0000000000000021fcca0d0a");
        assert!(Response::parse(&good).is_ok());
        good[12] ^= 1;
        assert_eq!(Response::parse(&good).unwrap_err(), PacketError::BadChecksum);
        assert!(matches!(Response::parse(&good[..10]), Err(PacketError::TooShort(10))));
        let mut cmd = hex("24c4001463fb4a6b0457000000000001fc940d0a");
        assert!(matches!(Response::parse(&cmd), Err(PacketError::BadMode(36))));
        cmd[0] = 42;
        cmd[3] = 0x15;
        assert!(matches!(Response::parse(&cmd), Err(PacketError::LengthMismatch { .. })));
    }
}
