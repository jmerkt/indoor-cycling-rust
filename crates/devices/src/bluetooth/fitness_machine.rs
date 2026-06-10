use crate::DeviceError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndoorBikeData {
    pub instantaneous_power_watts: Option<i16>,
    pub instantaneous_cadence_rpm: Option<u16>,
}

pub fn parse_indoor_bike_data(value: &[u8]) -> Result<IndoorBikeData, DeviceError> {
    let flags_bytes = value.get(..2).ok_or_else(|| {
        DeviceError::InvalidData("indoor bike data flags are incomplete".to_owned())
    })?;
    let flags = u16::from_le_bytes([flags_bytes[0], flags_bytes[1]]);
    let mut offset = 2;
    let mut data = IndoorBikeData::default();

    // Unlike most FTMS flags, bit 0 means instantaneous speed is omitted.
    if flags & (1 << 0) == 0 {
        offset = skip(value, offset, 2, "instantaneous speed")?;
    }
    if flags & (1 << 1) != 0 {
        offset = skip(value, offset, 2, "average speed")?;
    }
    if flags & (1 << 2) != 0 {
        let bytes = read(value, offset, 2, "instantaneous cadence")?;
        data.instantaneous_cadence_rpm = Some(u16::from_le_bytes([bytes[0], bytes[1]]) / 2);
        offset += 2;
    }
    if flags & (1 << 3) != 0 {
        offset = skip(value, offset, 2, "average cadence")?;
    }
    if flags & (1 << 4) != 0 {
        offset = skip(value, offset, 3, "total distance")?;
    }
    if flags & (1 << 5) != 0 {
        offset = skip(value, offset, 2, "resistance level")?;
    }
    if flags & (1 << 6) != 0 {
        let bytes = read(value, offset, 2, "instantaneous power")?;
        data.instantaneous_power_watts = Some(i16::from_le_bytes([bytes[0], bytes[1]]));
    }

    Ok(data)
}

fn skip(value: &[u8], offset: usize, length: usize, field: &str) -> Result<usize, DeviceError> {
    read(value, offset, length, field)?;
    Ok(offset + length)
}

fn read<'a>(
    value: &'a [u8],
    offset: usize,
    length: usize,
    field: &str,
) -> Result<&'a [u8], DeviceError> {
    value
        .get(offset..offset + length)
        .ok_or_else(|| DeviceError::InvalidData(format!("{field} is incomplete")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cadence_and_power() {
        let flags = (1_u16 << 2) | (1_u16 << 6);
        let value = [
            flags as u8,
            (flags >> 8) as u8,
            0x10,
            0x27, // speed
            180,
            0, // 90 rpm
            250,
            0, // 250 watts
        ];
        assert_eq!(
            parse_indoor_bike_data(&value).unwrap(),
            IndoorBikeData {
                instantaneous_power_watts: Some(250),
                instantaneous_cadence_rpm: Some(90),
            }
        );
    }
}
