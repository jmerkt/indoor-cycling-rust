use std::collections::HashMap;
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use tokio::time::sleep;
use uuid::Uuid;

use crate::bluetooth::uuids::{
    CYCLING_POWER_SERVICE, CYCLING_SPEED_AND_CADENCE_SERVICE, FITNESS_MACHINE_SERVICE,
    HEART_RATE_SERVICE,
};
use crate::{DeviceCapabilities, DeviceError, DeviceId, DiscoveredDevice};

#[derive(Debug, Clone, Default)]
pub struct BluetoothScanner {
    adapters: Vec<Adapter>,
    peripherals: HashMap<DeviceId, Peripheral>,
}

impl BluetoothScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn scan(&mut self, duration: Duration) -> Result<Vec<DiscoveredDevice>, DeviceError> {
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

        self.adapters = adapters;
        self.peripherals.clear();

        let mut devices = HashMap::new();
        for adapter in &self.adapters {
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

                let capabilities = classify_services(&properties.services);
                if capabilities.is_empty() {
                    continue;
                }

                let id = DeviceId::new(peripheral.id().to_string());
                let name = properties
                    .local_name
                    .unwrap_or_else(|| format!("Bluetooth device {id}"));
                self.peripherals.insert(id.clone(), peripheral);
                devices.insert(
                    id.clone(),
                    DiscoveredDevice {
                        id,
                        name,
                        capabilities,
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

    pub fn peripheral(&self, device_id: &DeviceId) -> Result<Peripheral, DeviceError> {
        self.peripherals.get(device_id).cloned().ok_or_else(|| {
            DeviceError::ConnectionFailed(format!(
                "device {device_id} is no longer available; scan for devices again"
            ))
        })
    }
}

fn classify_services(services: &[Uuid]) -> DeviceCapabilities {
    let fitness_machine = services.contains(&FITNESS_MACHINE_SERVICE);
    let cycling_power = services.contains(&CYCLING_POWER_SERVICE);
    let cycling_speed_and_cadence = services.contains(&CYCLING_SPEED_AND_CADENCE_SERVICE);

    DeviceCapabilities {
        power: fitness_machine || cycling_power,
        heart_rate: services.contains(&HEART_RATE_SERVICE),
        cadence: fitness_machine || cycling_power || cycling_speed_and_cadence,
        smart_trainer: fitness_machine,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_supported_services() {
        assert_eq!(
            classify_services(&[FITNESS_MACHINE_SERVICE]),
            DeviceCapabilities {
                power: true,
                cadence: true,
                smart_trainer: true,
                ..DeviceCapabilities::default()
            }
        );
        assert_eq!(
            classify_services(&[HEART_RATE_SERVICE]),
            DeviceCapabilities {
                heart_rate: true,
                ..DeviceCapabilities::default()
            }
        );
        assert_eq!(
            classify_services(&[CYCLING_POWER_SERVICE]),
            DeviceCapabilities {
                power: true,
                cadence: true,
                ..DeviceCapabilities::default()
            }
        );
        assert_eq!(
            classify_services(&[CYCLING_SPEED_AND_CADENCE_SERVICE]),
            DeviceCapabilities {
                cadence: true,
                ..DeviceCapabilities::default()
            }
        );
        assert!(classify_services(&[Uuid::nil()]).is_empty());
    }
}
