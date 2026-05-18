//! UBX message parser implementation.

// TODO:
// - Add tests.
// - Add function to convert from a typed messages eg. NavPosllh to Message.
// - Should the parser automaticly reset the state after finishing?
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

    Finished,
}

#[derive(Debug)]
#[allow(dead_code)]
struct Message<'a> {
    class: u8,
    id: u8,
    payload: &'a [u8],
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
    pub state: State,

    class: u8,
    id: u8,

    length: u16,


    // TODO: Remove payload from Parser and supply it separately in feed()
    payload: &'a mut [u8],
    
    // u16 instead of usize as UBX payload length can only be 2 bytes.
    payload_i: u16,

    // TODO: Store this as Checksum
    found_ck_a: u8,
    found_ck_b: u8,

    checksum: Checksum,
}

impl<'a> Parser<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            state: State::Sync1,
            class: 0,
            id: 0,
            length: 0,
            payload_i: 0,
            payload: buf,
            found_ck_a: 0,
            found_ck_b: 0,
            checksum: Checksum::new(),
        }
    }

    pub fn feed(&mut self, byte: u8) -> Result<Option<Message<'_>>, UbxError> {
        match self.state {
            State::Sync1 if byte == 0xb5 => {
                self.state = State::Sync2;
                Ok(None)
            }
            State::Sync1 => {
                self.state = State::Finished;
                Err(UbxError::Sync1Mismatch)
            }
            State::Sync2 if byte == 0x62 => {
                self.state = State::Class;
                Ok(None)
            }
            State::Sync2 => {
                self.state = State::Finished;
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
                    self.checksum(byte);
                    Err(UbxError::PayloadBufferOverflow)
                }
            }
            State::ChecksumA => {
                self.state = State::ChecksumB;
                self.found_ck_a = byte;
                Ok(None)
            }
            State::ChecksumB => {
                self.found_ck_b = byte;
                self.state = State::Finished;

                if self.checksum.a != self.found_ck_a || self.checksum.b != self.found_ck_b {
                    Err(UbxError::ChecksumMismatch)
                } else {
                    Ok(Some(Message {
                        class: self.class,
                        id: self.id,
                        payload: &self.payload[..self.length as usize],
                    }))
                }
            }
            State::Finished => {
                // TODO: This should reset automaticlly instead of panic!
                unimplemented!("called feed() on a finished parser, to reuse it set state = State::Sync1")
            }
        }
    }

    fn checksum(&mut self, byte: u8) {
        self.checksum.feed(byte);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Checksum {
    a: u8,
    b: u8,
}

impl Checksum {
    fn new() -> Self {
        Self { a: 0, b: 0 }
    }

    fn feed(&mut self, byte: u8) {
        self.a = self.a.wrapping_add(byte);
        self.b = self.b.wrapping_add(self.a);
    }
}

// TODO: Remove main() in favour of /examples
fn main() {
    let bytes = UBX_NAV_POSLLH;
    
    let mut buf = [0; 64];
    let mut parser = Parser::new(&mut buf);

    for byte in bytes {
        match parser.feed(byte) {
            Ok(Some(message)) => {
                let nav_posllh = match_nav_posllh_message(message);
                dbg!(nav_posllh);
            },
            // Ok(None) => _ = dbg!(parser.state),
            Ok(None) => {}
            Err(err) => {
                // TODO: The parser should do this automatically?
                parser.state = State::Sync1;
                dbg!(err);
            }
        }
    }

    // dbg!(parser);
}

// TODO: Add conversion to Message struct
macro_rules! ubx_message {
    (struct $name:ident {
        $(
            $field:ident: $Ty:path
        ),*
        $(,)?
    }) => {
    
        #[derive(Debug, Clone, Copy)]
        #[allow(dead_code)]
        struct $name {
            $(
                $field: $Ty,
            )*
        }
        
        impl $name {
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
    };
}

ubx_message! {
    struct NavPosllh {
        itow: u32,
        lon: i32,
        lat: i32,
        height: i32,
        hmsl: i32,
        hacc: u32,
        vacc: u32,
    }
}

fn match_nav_posllh_message(m: Message<'_>) -> Option<NavPosllh> {
    match m {
        Message {
            class: 0x01,
            id: 0x02,
            payload,
        } => {
            NavPosllh::from_bytes(payload)
        }
        _ => None,
    }
}

pub const UBX_NAV_POSLLH: [u8; 36] = [
    0xb5, 0x62,                 // Sync chars
    0x01, 0x02,                 // CLASS = NAV, ID = POSLLH
    0x1c, 0x00,                 // Payload length = 28 bytes

    // Payload
    0xe0, 0x93, 0x04, 0x00,     // iTOW
    0x08, 0xfe, 0x83, 0x16,     // Longitude
    0x30, 0x48, 0x08, 0x18,     // Latitude
    0x5c, 0x1b, 0x0a, 0x00,     // Height above ellipsoid
    0xa0, 0x19, 0x0a, 0x00,     // Height above MSL
    0x34, 0x12, 0x00, 0x00,     // Horizontal accuracy
    0x78, 0x56, 0x00, 0x00,     // Vertical accuracy

    // Checksum
    0x25, 0xc9,
];
