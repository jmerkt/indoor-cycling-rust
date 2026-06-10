use std::collections::HashMap;
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use tokio::time::sleep;
use uuid::Uuid;

use crate::bluetooth::uuids::{FITNESS_MACHINE_SERVICE, HEART_RATE_SERVICE};
use crate::{DeviceError, DeviceId, DeviceKind, DiscoveredDevice};

#[derive(Debug, Clone, Default)]
pub struct BluetoothScanner;

impl BluetoothScanner {
    pub fn new() -> Self {
        Self
    }

    pub async fn scan(&self, duration: Duration) -> Result<Vec<DiscoveredDevice>, DeviceError> {
        let manager = Manager::new()
            .await
            .map_err(|error| DeviceError::BluetoothUnavailable(error.to_string()))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|error| DeviceError::BluetoothUnavailable(error.to_string()))?;

        if adapters.is_empty() {
            return Err(DeviceError::BluetoothUnavailable(
                "no Bluetooth adapter was found".to_owned(),
            ));
        }

        let mut devices = HashMap::new();
        for adapter in adapters {
            adapter
                .start_scan(ScanFilter::default())
                .await
                .map_err(|error| DeviceError::ScanFailed(error.to_string()))?;
            sleep(duration).await;

            let peripherals = adapter
                .peripherals()
                .await
                .map_err(|error| DeviceError::ScanFailed(error.to_string()))?;

            for peripheral in peripherals {
                let Some(properties) = peripheral
                    .properties()
                    .await
                    .map_err(|error| DeviceError::ScanFailed(error.to_string()))?
                else {
                    continue;
                };

                let Some(kind) = classify_services(&properties.services) else {
                    continue;
                };

                let id = DeviceId::new(peripheral.id().to_string());
                let name = properties
                    .local_name
                    .unwrap_or_else(|| format!("Bluetooth device {id}"));
                devices.insert(
                    id.clone(),
                    DiscoveredDevice {
                        id,
                        name,
                        kind,
                        signal_strength: properties.rssi,
                    },
                );
            }

            adapter
                .stop_scan()
                .await
                .map_err(|error| DeviceError::ScanFailed(error.to_string()))?;
        }

        let mut devices: Vec<_> = devices.into_values().collect();
        devices.sort_by(|left, right| {
            right
                .signal_strength
                .cmp(&left.signal_strength)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(devices)
    }
}

fn classify_services(services: &[Uuid]) -> Option<DeviceKind> {
    let is_trainer = services.contains(&FITNESS_MACHINE_SERVICE);
    let is_heart_rate_monitor = services.contains(&HEART_RATE_SERVICE);

    match (is_trainer, is_heart_rate_monitor) {
        (true, true) => Some(DeviceKind::TrainerAndHeartRateMonitor),
        (true, false) => Some(DeviceKind::Trainer),
        (false, true) => Some(DeviceKind::HeartRateMonitor),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_supported_services() {
        assert_eq!(
            classify_services(&[FITNESS_MACHINE_SERVICE]),
            Some(DeviceKind::Trainer)
        );
        assert_eq!(
            classify_services(&[HEART_RATE_SERVICE]),
            Some(DeviceKind::HeartRateMonitor)
        );
        assert_eq!(
            classify_services(&[FITNESS_MACHINE_SERVICE, HEART_RATE_SERVICE]),
            Some(DeviceKind::TrainerAndHeartRateMonitor)
        );
        assert_eq!(classify_services(&[Uuid::nil()]), None);
    }
}
