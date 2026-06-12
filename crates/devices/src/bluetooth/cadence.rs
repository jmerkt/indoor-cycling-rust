use crate::DeviceError;

#[derive(Debug, Clone, Copy, Default)]
pub struct CadenceTracker {
    previous: Option<CrankData>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrankData {
    pub cumulative_revolutions: u16,
    pub event_time_1024: u16,
}

impl CadenceTracker {
    pub fn update(&mut self, current: CrankData) -> Option<u16> {
        let previous = self.previous.replace(current)?;
        let revolutions = current
            .cumulative_revolutions
            .wrapping_sub(previous.cumulative_revolutions);
        let event_ticks = current
            .event_time_1024
            .wrapping_sub(previous.event_time_1024);

        if revolutions == 0 || event_ticks == 0 {
            return None;
        }

        let rpm = u32::from(revolutions) * 60 * 1024 / u32::from(event_ticks);
        u16::try_from(rpm).ok()
    }
}

pub fn parse_csc_measurement(value: &[u8]) -> Result<Option<CrankData>, DeviceError> {
    let flags = *value
        .first()
        .ok_or_else(|| DeviceError::InvalidData("CSC measurement is empty".to_owned()))?;
    let mut offset = 1;

    if flags & 0x01 != 0 {
        read(value, offset, 6, "CSC wheel revolution data")?;
        offset += 6;
    }

    if flags & 0x02 == 0 {
        return Ok(None);
    }

    let bytes = read(value, offset, 4, "CSC crank revolution data")?;
    Ok(Some(CrankData {
        cumulative_revolutions: u16::from_le_bytes([bytes[0], bytes[1]]),
        event_time_1024: u16::from_le_bytes([bytes[2], bytes[3]]),
    }))
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
    fn calculates_cadence_from_consecutive_crank_events() {
        let mut tracker = CadenceTracker::default();
        assert_eq!(
            tracker.update(CrankData {
                cumulative_revolutions: 10,
                event_time_1024: 1_000,
            }),
            None
        );
        assert_eq!(
            tracker.update(CrankData {
                cumulative_revolutions: 11,
                event_time_1024: 2_024,
            }),
            Some(60)
        );
    }

    #[test]
    fn parses_csc_crank_data_after_wheel_data() {
        let value = [
            0x03, 1, 0, 0, 0, 2, 0, // wheel data
            12, 0, 0x00, 0x04, // crank data
        ];
        assert_eq!(
            parse_csc_measurement(&value).unwrap(),
            Some(CrankData {
                cumulative_revolutions: 12,
                event_time_1024: 1024,
            })
        );
    }
}
