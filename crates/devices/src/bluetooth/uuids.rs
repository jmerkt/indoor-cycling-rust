use uuid::Uuid;

const BLUETOOTH_BASE_UUID_SUFFIX: u128 = 0x0000_0000_0000_1000_8000_0080_5f9b_34fb;

pub const FITNESS_MACHINE_SERVICE: Uuid = bluetooth_uuid(0x1826);
pub const HEART_RATE_SERVICE: Uuid = bluetooth_uuid(0x180d);
pub const HEART_RATE_MEASUREMENT: Uuid = bluetooth_uuid(0x2a37);

const fn bluetooth_uuid(short_uuid: u16) -> Uuid {
    Uuid::from_u128(((short_uuid as u128) << 96) | BLUETOOTH_BASE_UUID_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_assigned_numbers_to_bluetooth_uuids() {
        assert_eq!(
            FITNESS_MACHINE_SERVICE.to_string(),
            "00001826-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            HEART_RATE_SERVICE.to_string(),
            "0000180d-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            HEART_RATE_MEASUREMENT.to_string(),
            "00002a37-0000-1000-8000-00805f9b34fb"
        );
    }
}
