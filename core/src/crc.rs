/// CRC-16/CCITT-FALSE: polynomial 0x1021, initial value 0xFFFF, no reflection.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::crc16;
    #[test]
    fn check_value() {
        // Standard check value for CRC-16/CCITT-FALSE over "123456789".
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }
}
