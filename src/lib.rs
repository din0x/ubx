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

/// Raw UBX message returned by the [`Parser`].
#[derive(Debug, Clone, Copy)]
pub struct Message<'a> {
    pub class: u8,
    pub id: u8,
    pub payload: &'a [u8],
}

impl<'a> Message<'a> {
    /// Returns an iterator of bytes in the message.
    // TODO: Document that the iterator yields sync and checksum bytes
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

/// An iterator of bytes in a UBX message.
pub struct Bytes<'a> {
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

/// The error return by the parser when a packet is malformed.
#[derive(Debug)]
pub enum UbxError {
    Sync1Mismatch,
    Sync2Mismatch,
    PayloadBufferOverflow,
    ChecksumMismatch,
}

/// [UBX](https://docs.sparkfun.com/SparkFun_GNSS_Flex_System/SparkPNT_GNSS_Flex_Module_DAN-F10N/ubx_protocol/) parser.
#[derive(Debug)]
pub struct Parser<'a> {
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
    /// Returns a parser given a payload buffer.
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

    /// Feeds a byte to the parser.
    /// 
    /// When the UBX packet is malformed this method will return an [`Err`] and reset
    /// the parser. No special handling of the error is needed.
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

/// Represents the checksum of a UBX packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Checksum {
    pub a: u8,
    pub b: u8,
}

impl Checksum {
    /// Returns a zeroed checksum.
    pub fn new() -> Self {
        Self { a: 0, b: 0 }
    }

    /// Returns a checksum given an iterator of bytes.
    pub fn from_iter(i: impl IntoIterator<Item = u8>) -> Self {
        let mut checksum = Self::new();
        i.into_iter().for_each(|b| checksum.feed(b));
        checksum
    }

    /// Updates the checksum given a byte.
    pub fn feed(&mut self, byte: u8) {
        self.a = self.a.wrapping_add(byte);
        self.b = self.b.wrapping_add(self.a);
    }
}

/// Defines the supported UBX protocol subset.
///
/// Most of the time you only care about a few UBX messages. This macro allows you to define
/// which messages are captured. Useful for minimizing codegen.
#[macro_export]
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
                        } => $Message::from_payload(payload).map(Self::$Message).ok_or(TryFromMessageError(())),
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
                pub $field: $Ty,
            )*
        }

        impl $name {
            pub const CLASS: u8 = $class;
            pub const ID: u8 = $id;

            pub fn from_payload(mut payload: &[u8]) -> Option<Self> {
                $(
                    let arr = *payload.get(..::core::mem::size_of::<$Ty>())?.as_array::<{ ::core::mem::size_of::<$Ty>() }>()?;

                    let $field: $Ty = <$Ty>::from_le_bytes(arr);
                    payload = &payload.get(::core::mem::size_of::<$Ty>()..)?;
                )*

                _ = payload;

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
                    } => Self::from_payload(payload).ok_or(TryFromMessageError(())),
                    _ => Err(TryFromMessageError(())),
                }
            }
        }
    };
}

/// The error type return when conversion from a raw [`Message`] to typed UBX message fails.
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
