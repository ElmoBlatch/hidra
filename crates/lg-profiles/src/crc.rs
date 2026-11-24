/// CRC-CCITT (polynomial 0x1021, initial value 0xFFFF, no reflection, no
/// final XOR) as used for onboard-profile sectors. Stored big-endian in the
/// last two bytes of each sector.
pub fn crc_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    #[test]
    fn check_value() {
        // Standard CRC-16/CCITT-FALSE check value.
        assert_eq!(super::crc_ccitt(b"123456789"), 0x29b1);
    }
}
