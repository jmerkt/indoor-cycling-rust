use crate::bluetooth::cadence::CrankData;
use crate::DeviceError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CyclingPowerMeasurement {
    pub instantaneous_power_watts: i16,
    pub crank_data: Option<CrankData>,
}

pub fn parse_measurement(value: &[u8]) -> Result<CyclingPowerMeasurement, DeviceError> {
    let header = value.get(..4).ok_or_else(|| {
        DeviceError::InvalidData("cycling power measurement header is incomplete".to_owned())
    })?;
    let flags = u16::from_le_bytes([header[0], header[1]]);
    let instantaneous_power_watts = i16::from_le_bytes([header[2], header[3]]);
    let mut offset = 4;

    if flags & (1 << 0) != 0 {
        offset = skip(value, offset, 1, "pedal power balance")?;
    }
    if flags & (1 << 2) != 0 {
        offset = skip(value, offset, 2, "accumulated torque")?;
    }
    if flags & (1 << 4) != 0 {
        offset = skip(value, offset, 6, "wheel revolution data")?;
    }

    let crank_data = if flags & (1 << 5) != 0 {
        let bytes = value.get(offset..offset + 4).ok_or_else(|| {
            DeviceError::InvalidData("cycling power crank data is incomplete".to_owned())
        })?;
        Some(CrankData {
            cumulative_revolutions: u16::from_le_bytes([bytes[0], bytes[1]]),
            event_time_1024: u16::from_le_bytes([bytes[2], bytes[3]]),
        })
    } else {
        None
    };

    Ok(CyclingPowerMeasurement {
        instantaneous_power_watts,
        crank_data,
    })
}

fn skip(value: &[u8], offset: usize, length: usize, field: &str) -> Result<usize, DeviceError> {
    value
        .get(offset..offset + length)
        .ok_or_else(|| DeviceError::InvalidData(format!("{field} is incomplete")))?;
    Ok(offset + length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_power_without_optional_fields() {
        assert_eq!(
            parse_measurement(&[0, 0, 0xfa, 0]).unwrap(),
            CyclingPowerMeasurement {
                instantaneous_power_watts: 250,
                crank_data: None,
            }
        );
    }

    #[test]
    fn parses_power_and_crank_data() {
        assert_eq!(
            parse_measurement(&[0x20, 0, 200, 0, 10, 0, 0, 4]).unwrap(),
            CyclingPowerMeasurement {
                instantaneous_power_watts: 200,
                crank_data: Some(CrankData {
                    cumulative_revolutions: 10,
                    event_time_1024: 1024,
                }),
            }
        );
    }
}
