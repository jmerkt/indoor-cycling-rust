use crate::DeviceError;

pub fn parse_measurement(value: &[u8]) -> Result<u16, DeviceError> {
    let Some(flags) = value.first().copied() else {
        return Err(DeviceError::InvalidData(
            "heart rate measurement is empty".to_owned(),
        ));
    };

    if flags & 0x01 == 0 {
        value
            .get(1)
            .copied()
            .map(u16::from)
            .ok_or_else(|| DeviceError::InvalidData("8-bit heart rate value is missing".to_owned()))
    } else {
        let bytes = value.get(1..3).ok_or_else(|| {
            DeviceError::InvalidData("16-bit heart rate value is incomplete".to_owned())
        })?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_8_bit_heart_rate() {
        assert_eq!(parse_measurement(&[0x00, 147]).unwrap(), 147);
    }

    #[test]
    fn parses_16_bit_heart_rate() {
        assert_eq!(parse_measurement(&[0x01, 0x2c, 0x01]).unwrap(), 300);
    }

    #[test]
    fn rejects_incomplete_measurements() {
        assert!(matches!(
            parse_measurement(&[0x01, 0x2c]),
            Err(DeviceError::InvalidData(_))
        ));
    }
}
