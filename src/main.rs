//! UBX message parser implementation.

// TODO:
// - Add tests.
// - Implement all UBX messages using ubx_message!.
// - Split into modules (lib.rs, messages.rs, parser.rs).
// - Publish as a crate.

#[derive(Debug, Copy, Clone)]
enum State {
    Sync1,
    Sync2,

    Class,
    Id,

    Length0,
    Length1,
    Payload,

    ChecksumA,
    ChecksumB,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct Message<'a> {
    class: u8,
    id: u8,
    payload: &'a [u8],
}

impl<'a> Message<'a> {
    pub fn into_bytes(self) -> Bytes<'a> {
        Bytes {
            state: State::Sync1,
            payload_i: 0,
            message: self,
            checksum: Checksum::new(),
            finished: false,
        }
    }
}

struct Bytes<'a> {
    state: State,
    payload_i: u16,
    message: Message<'a>,
    checksum: Checksum,
    finished: bool,
}

impl Iterator for Bytes<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        match self.state {
            State::Sync1 => {
                self.state = State::Sync2;
                Some(0xb5)
            }
            State::Sync2 => {
                self.state = State::Class;
                Some(0x62)
            }
            State::Class => {
                self.state = State::Id;
                self.checksum.feed(self.message.class);
                Some(self.message.class)
            }
            State::Id => {
                self.state = State::Length0;
                self.checksum.feed(self.message.id);
                Some(self.message.id)
            }
            State::Length0 => {
                let byte = (self.message.payload.len() & 0xff) as u8;
                self.state = State::Length1;

                self.checksum.feed(byte);
                Some(byte)
            }
            State::Length1 => {
                if self.message.payload.len() == 0 {
                    self.state = State::ChecksumA;
                } else {
                    self.state = State::Payload;
                }

                let byte = ((self.message.payload.len() >> 8) & 0xff) as u8;

                self.checksum.feed(byte);
                Some(byte)
            }
            State::Payload => {
                let Some(&byte) = self.message.payload.get(self.payload_i as usize) else {
                    unreachable!();
                };

                self.payload_i += 1;

                if self.message.payload.len() == self.payload_i as usize {
                    self.state = State::ChecksumA;
                }

                self.checksum.feed(byte);
                Some(byte)
            }
            State::ChecksumA => {
                self.state = State::ChecksumB;
                Some(self.checksum.a)
            }
            State::ChecksumB => {
                self.finished = true;
                Some(self.checksum.b)
            }
        }
    }
}

#[derive(Debug)]
enum UbxError {
    Sync1Mismatch,
    Sync2Mismatch,
    PayloadBufferOverflow,
    ChecksumMismatch,
}

#[derive(Debug)]
struct Parser<'a> {
    state: State,
    checksum: Checksum,

    class: u8,
    id: u8,

    length: u16,

    // TODO: Remove payload from Parser and supply it separately in feed()
    payload: &'a mut [u8],
    // u16 instead of usize as UBX payload length can only be 2 bytes.
    payload_i: u16,

    found_checksum: Checksum,
}

impl<'a> Parser<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            state: State::Sync1,
            checksum: Checksum::new(),

            class: 0,
            id: 0,
            length: 0,
            payload_i: 0,
            payload: buf,
            found_checksum: Checksum::new(),
        }
    }

    fn reset(&mut self) {
        self.state = State::Sync1;
        self.checksum = Checksum::new();
    }

    pub fn feed(&mut self, byte: u8) -> Result<Option<Message<'_>>, UbxError> {
        match self.state {
            State::Sync1 if byte == 0xb5 => {
                self.state = State::Sync2;
                Ok(None)
            }
            State::Sync1 => {
                self.reset();
                Err(UbxError::Sync1Mismatch)
            }
            State::Sync2 if byte == 0x62 => {
                self.state = State::Class;
                Ok(None)
            }
            State::Sync2 => {
                self.reset();
                Err(UbxError::Sync2Mismatch)
            }
            State::Class => {
                self.checksum(byte);

                self.class = byte;
                self.state = State::Id;
                Ok(None)
            }
            State::Id => {
                self.checksum(byte);

                self.id = byte;
                self.state = State::Length0;
                Ok(None)
            }
            State::Length0 => {
                self.checksum(byte);

                self.length = byte as u16;
                self.state = State::Length1;
                Ok(None)
            }
            State::Length1 => {
                self.checksum(byte);

                self.state = State::Payload;
                self.length |= (byte as u16) << 8;
                self.payload_i = 0;

                if self.length == 0 {
                    self.state = State::ChecksumA;
                }

                Ok(None)
            }
            State::Payload => {
                if let Some(b) = self.payload.get_mut(self.payload_i as usize) {
                    *b = byte;
                    self.checksum(byte);
                    self.payload_i += 1;

                    if self.payload_i == self.length {
                        self.state = State::ChecksumA;
                    }

                    Ok(None)
                } else {
                    // When the provieded buffer is too small, we have two choices:
                    // - Continue to parse the message and then retturn the error or
                    // - Return the error immediately and reset() the parser
                    //
                    // TODO: For now we return the error and reset(), but it might
                    // be better to parse the entire message.

                    self.reset();
                    Err(UbxError::PayloadBufferOverflow)
                }
            }
            State::ChecksumA => {
                self.state = State::ChecksumB;
                self.found_checksum.a = byte;
                Ok(None)
            }
            State::ChecksumB => {
                self.found_checksum.b = byte;

                if self.checksum != self.found_checksum {
                    self.reset();
                    Err(UbxError::ChecksumMismatch)
                } else {
                    self.reset();
                    Ok(Some(Message {
                        class: self.class,
                        id: self.id,
                        payload: &self.payload[..self.length as usize],
                    }))
                }
            }
        }
    }

    fn checksum(&mut self, byte: u8) {
        self.checksum.feed(byte);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Checksum {
    a: u8,
    b: u8,
}

impl Checksum {
    pub fn new() -> Self {
        Self { a: 0, b: 0 }
    }

    pub fn from_iter(i: impl IntoIterator<Item = u8>) -> Self {
        let mut checksum = Self::new();
        i.into_iter().for_each(|b| checksum.feed(b));
        checksum
    }

    pub fn feed(&mut self, byte: u8) {
        self.a = self.a.wrapping_add(byte);
        self.b = self.b.wrapping_add(self.a);
    }
}

// TODO: Remove main() in favour of /examples
fn main() {
    // let message = Message {
    //     class: 0xf0,
    //     id: 0xf1,
    //     payload: &[0x1, 0x2, 0x3, 0x4],
    // };

    // let bytes: Vec<u8> = message.into_bytes().collect();

    let mut bytes = Vec::new();
    bytes.push(0x12);
    bytes.extend(UBX_NAV_POSLLH);
    bytes.extend(UBX_NAV_POSLLH);
    bytes.push(0x67);
    bytes.push(0x12);
    bytes.push(0x3);
    bytes.extend(UBX_NAV_POSLLH);
    bytes.extend(UBX_NAV_POSLLH);

    let mut buf = [0; 64];
    let mut parser = Parser::new(&mut buf);

    for byte in bytes {
        match parser.feed(byte) {
            Ok(Some(message)) => {
                let nav_posllh = Proto::try_from(message);
                _ = dbg!(nav_posllh);
            }
            Ok(None) => {}
            Err(err) => {
                dbg!(err);
            }
        }
    }
}

macro_rules! proto {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $Message:ident
            ),*
            $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $(
                $Message($Message),
            )*
        }

        impl<'a> ::core::convert::TryFrom<Message<'a>> for $name {
            type Error = TryFromMessageError;

            fn try_from(value: Message<'a>) -> Result<$name, Self::Error> {
                match value {
                    $(
                        Message {
                            class: $Message::CLASS,
                            id: $Message::ID,
                            payload,
                        } => $Message::from_bytes(payload).map(Self::$Message).ok_or(TryFromMessageError(())),
                    )*
                    _ => Err(TryFromMessageError(())),
                }
            }
        }
    };
}

proto! {
    #[derive(Debug)]
    pub enum Proto {
        NavPosllh,
        NavStatus,
        NavVelNed,
        NavTimeUtc,
    }
}

// TODO: Add conversion to Message struct
macro_rules! message {
    (
    const CLASS = $class:literal;
    const ID = $id:literal;

    $(#[$meta:meta])*
    $vis:vis struct $name:ident {
        $(
            $field:ident: $Ty:path
        ),*
        $(,)?
    }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                $field: $Ty,
            )*
        }

        impl $name {
            pub const CLASS: u8 = $class;
            pub const ID: u8 = $id;

            pub fn from_bytes(mut bytes: &[u8]) -> Option<Self> {
                $(
                    let arr = *bytes.get(..::core::mem::size_of::<$Ty>())?.as_array::<{ ::core::mem::size_of::<$Ty>() }>()?;

                    let $field: $Ty = <$Ty>::from_le_bytes(arr);
                    bytes = &bytes.get(::core::mem::size_of::<$Ty>()..)?;
                )*

                _ = bytes;

                Some(Self {
                    $(
                        $field,
                    )*
                })
            }
        }

        impl<'a> ::core::convert::TryFrom<Message<'a>> for $name {
            type Error = TryFromMessageError;

            fn try_from(value: Message<'a>) -> Result<$name, Self::Error> {
                match value {
                    Message {
                        class: Self::CLASS,
                        id: Self::ID,
                        payload,
                    } => Self::from_bytes(payload).ok_or(TryFromMessageError(())),
                    _ => Err(TryFromMessageError(())),
                }
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TryFromMessageError(());

message! {
    const CLASS = 0x01;
    const ID = 0x02;

    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    pub struct NavPosllh {
        itow: u32,
        lon: i32,
        lat: i32,
        height: i32,
        hmsl: i32,
        hacc: u32,
        vacc: u32,
    }
}

message! {
    const CLASS = 0x01;
    const ID = 0x03;

    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    pub struct NavStatus {
        itow: u32,
        gps_fix: u8,
        flags: u8,
        fix_stat: u8,
        flags2: u8,
        ttff: u32,
        msss: u32,
    }
}

message! {
    const CLASS = 0x01;
    const ID = 0x12;

    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    pub struct NavVelNed {
        itow: u32,
        vel_n: i32,
        vel_e: i32,
        vel_d: i32,
        speed: u32,
        gspeed: u32,
        heading: i32,
        sacc: u32,
        cacc: u32,
    }
}

message! {
    const CLASS = 0x01;
    const ID = 0x21;

    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    pub struct NavTimeUtc {
        itow: u32,
        tacc: u32,
        nano: i32,
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        min: u8,
        sec: u8,
        valid: u8,
    }
}

pub const UBX_NAV_POSLLH: [u8; 36] = [
    0xb5, 0x62, //              sync
    0x01, 0x02, //              class = NAV, id = POSLLH
    0x1c, 0x00, //              length = 28 bytes
    0xe0, 0x93, 0x04, 0x00, //  itow
    0x08, 0xfe, 0x83, 0x16, //  long
    0x30, 0x48, 0x08, 0x18, //  lat
    0x5c, 0x1b, 0x0a, 0x00, //  height
    0xa0, 0x19, 0x0a, 0x00, //  hmsl
    0x34, 0x12, 0x00, 0x00, //  hacc
    0x78, 0x56, 0x00, 0x00, //  vacc
    0x25, 0xc9, //              checksum
];
