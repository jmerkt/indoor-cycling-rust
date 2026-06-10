use std::pin::Pin;

use btleplug::api::{Central, Manager as _, Peripheral as _, ValueNotification};
use btleplug::platform::{Manager, Peripheral};
use futures::Stream;

use crate::bluetooth::uuids::{HEART_RATE_MEASUREMENT, HEART_RATE_SERVICE};
use crate::{DeviceError, DeviceId};

pub struct HeartRateConnection {
    pub device_id: DeviceId,
    pub peripheral: Peripheral,
    pub notifications: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
}

pub async fn connect(device_id: DeviceId) -> Result<HeartRateConnection, DeviceError> {
    let manager = Manager::new()
        .await
        .map_err(|error| DeviceError::BluetoothUnavailable(error.to_string()))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|error| DeviceError::BluetoothUnavailable(error.to_string()))?;

    let mut matching_peripheral = None;
    for adapter in adapters {
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| DeviceError::ConnectionFailed(error.to_string()))?;
        matching_peripheral = peripherals
            .into_iter()
            .find(|peripheral| peripheral.id().to_string() == device_id.as_str());
        if matching_peripheral.is_some() {
            break;
        }
    }

    let peripheral = matching_peripheral.ok_or_else(|| {
        DeviceError::ConnectionFailed(format!("device {device_id} is no longer available"))
    })?;

    if !peripheral
        .is_connected()
        .await
        .map_err(|error| DeviceError::ConnectionFailed(error.to_string()))?
    {
        peripheral
            .connect()
            .await
            .map_err(|error| DeviceError::ConnectionFailed(error.to_string()))?;
    }

    if let Err(error) = peripheral.discover_services().await {
        let _ = peripheral.disconnect().await;
        return Err(DeviceError::ConnectionFailed(error.to_string()));
    }

    let characteristic = peripheral
        .characteristics()
        .into_iter()
        .find(|characteristic| {
            characteristic.service_uuid == HEART_RATE_SERVICE
                && characteristic.uuid == HEART_RATE_MEASUREMENT
        });
    let Some(characteristic) = characteristic else {
        let _ = peripheral.disconnect().await;
        return Err(DeviceError::ConnectionFailed(
            "heart rate measurement characteristic was not found".to_owned(),
        ));
    };

    if let Err(error) = peripheral.subscribe(&characteristic).await {
        let _ = peripheral.disconnect().await;
        return Err(DeviceError::ConnectionFailed(error.to_string()));
    }

    let notifications = match peripheral.notifications().await {
        Ok(notifications) => notifications,
        Err(error) => {
            let _ = peripheral.disconnect().await;
            return Err(DeviceError::ConnectionFailed(error.to_string()));
        }
    };

    Ok(HeartRateConnection {
        device_id,
        peripheral,
        notifications,
    })
}

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
