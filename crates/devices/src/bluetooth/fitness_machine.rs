use crate::DeviceError;

pub const REQUEST_CONTROL_OPCODE: u8 = 0x00;
pub const RESET_OPCODE: u8 = 0x01;
pub const SET_TARGET_POWER_OPCODE: u8 = 0x05;
const RESPONSE_CODE_OPCODE: u8 = 0x80;
const SUCCESS_RESULT_CODE: u8 = 0x01;

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

pub fn request_control_command() -> [u8; 1] {
    [REQUEST_CONTROL_OPCODE]
}

pub fn reset_command() -> [u8; 1] {
    [RESET_OPCODE]
}

pub fn set_target_power_command(watts: u16) -> [u8; 3] {
    let watts = i16::try_from(watts).unwrap_or(i16::MAX).to_le_bytes();
    [SET_TARGET_POWER_OPCODE, watts[0], watts[1]]
}

pub fn parse_control_point_response(value: &[u8], expected_opcode: u8) -> Result<(), DeviceError> {
    let response = value.get(..3).ok_or_else(|| {
        DeviceError::InvalidData("fitness machine control response is incomplete".to_owned())
    })?;

    if response[0] != RESPONSE_CODE_OPCODE {
        return Err(DeviceError::InvalidData(format!(
            "unexpected fitness machine response opcode 0x{:02x}",
            response[0]
        )));
    }
    if response[1] != expected_opcode {
        return Err(DeviceError::InvalidData(format!(
            "fitness machine responded to opcode 0x{:02x}, expected 0x{expected_opcode:02x}",
            response[1]
        )));
    }
    if response[2] != SUCCESS_RESULT_CODE {
        return Err(DeviceError::ControlFailed(
            control_result_name(response[2]).to_owned(),
        ));
    }

    Ok(())
}

fn control_result_name(result_code: u8) -> &'static str {
    match result_code {
        0x02 => "operation not supported",
        0x03 => "invalid parameter",
        0x04 => "operation failed",
        0x05 => "control not permitted",
        _ => "unknown fitness machine control error",
    }
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

    #[test]
    fn builds_target_power_command() {
        assert_eq!(set_target_power_command(250), [0x05, 0xfa, 0x00]);
    }

    #[test]
    fn accepts_successful_control_response() {
        assert_eq!(
            parse_control_point_response(&[0x80, 0x05, 0x01], SET_TARGET_POWER_OPCODE),
            Ok(())
        );
    }

    #[test]
    fn reports_rejected_control_response() {
        assert_eq!(
            parse_control_point_response(&[0x80, 0x05, 0x05], SET_TARGET_POWER_OPCODE),
            Err(DeviceError::ControlFailed(
                "control not permitted".to_owned()
            ))
        );
    }
}
