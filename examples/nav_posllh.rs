//! Example of parsing NavPosllh packets.

use ubx::{Parser, NavPosllh};

fn main() {
    let mut bytes = Vec::new();
    bytes.push(0x12); // Random noise
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
                let nav_posllh = NavPosllh::try_from(message);
                _ = dbg!(nav_posllh);
            }
            Ok(None) => {}
            Err(err) => {
                dbg!(err);
            }
        }
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
