use std::pin::Pin;

use btleplug::api::{Peripheral as _, ValueNotification};
use btleplug::platform::Peripheral;
use futures::Stream;

use crate::bluetooth::cadence::{self, CadenceTracker};
use crate::bluetooth::cycling_power;
use crate::bluetooth::fitness_machine;
use crate::bluetooth::heart_rate;
use crate::bluetooth::uuids::{
    CSC_MEASUREMENT, CYCLING_POWER_MEASUREMENT, CYCLING_POWER_SERVICE,
    CYCLING_SPEED_AND_CADENCE_SERVICE, FITNESS_MACHINE_SERVICE, HEART_RATE_MEASUREMENT,
    HEART_RATE_SERVICE, INDOOR_BIKE_DATA,
};
use crate::{DeviceError, DeviceId, DeviceMeasurement, DeviceRole};

pub struct DeviceConnection {
    pub role: DeviceRole,
    pub device_id: DeviceId,
    pub peripheral: Peripheral,
    pub notifications: Option<Pin<Box<dyn Stream<Item = ValueNotification> + Send>>>,
    cadence_tracker: CadenceTracker,
}

impl DeviceConnection {
    pub fn parse_notification(
        &mut self,
        notification: &ValueNotification,
    ) -> Result<Option<DeviceMeasurement>, DeviceError> {
        match self.role {
            DeviceRole::HeartRate if notification.uuid == HEART_RATE_MEASUREMENT => {
                heart_rate::parse_measurement(&notification.value)
                    .map(DeviceMeasurement::HeartRateBpm)
                    .map(Some)
            }
            DeviceRole::PowerSource | DeviceRole::SecondaryPowerSource
                if notification.uuid == CYCLING_POWER_MEASUREMENT =>
            {
                cycling_power::parse_measurement(&notification.value)
                    .map(|value| DeviceMeasurement::PowerWatts(value.instantaneous_power_watts))
                    .map(Some)
            }
            DeviceRole::PowerSource | DeviceRole::SecondaryPowerSource
                if notification.uuid == INDOOR_BIKE_DATA =>
            {
                fitness_machine::parse_indoor_bike_data(&notification.value).map(|value| {
                    value
                        .instantaneous_power_watts
                        .map(DeviceMeasurement::PowerWatts)
                })
            }
            DeviceRole::Cadence if notification.uuid == CSC_MEASUREMENT => {
                let crank_data = cadence::parse_csc_measurement(&notification.value)?;
                Ok(crank_data
                    .and_then(|value| self.cadence_tracker.update(value))
                    .map(DeviceMeasurement::CadenceRpm))
            }
            DeviceRole::Cadence if notification.uuid == CYCLING_POWER_MEASUREMENT => {
                let measurement = cycling_power::parse_measurement(&notification.value)?;
                Ok(measurement
                    .crank_data
                    .and_then(|value| self.cadence_tracker.update(value))
                    .map(DeviceMeasurement::CadenceRpm))
            }
            DeviceRole::Cadence if notification.uuid == INDOOR_BIKE_DATA => {
                fitness_machine::parse_indoor_bike_data(&notification.value).map(|value| {
                    value
                        .instantaneous_cadence_rpm
                        .map(DeviceMeasurement::CadenceRpm)
                })
            }
            _ => Ok(None),
        }
    }
}

pub async fn connect(
    role: DeviceRole,
    device_id: DeviceId,
    peripheral: Peripheral,
) -> Result<DeviceConnection, DeviceError> {
    let was_connected = peripheral.is_connected().await.map_err(connection_failed)?;
    if !was_connected {
        peripheral.connect().await.map_err(connection_failed)?;
    }

    if let Err(error) = peripheral.discover_services().await {
        disconnect_if_owned(&peripheral, was_connected).await;
        return Err(connection_failed(error));
    }

    let characteristics = peripheral.characteristics();
    let characteristic = match role {
        DeviceRole::PowerSource | DeviceRole::SecondaryPowerSource => characteristics
            .iter()
            .find(|value| {
                value.service_uuid == CYCLING_POWER_SERVICE
                    && value.uuid == CYCLING_POWER_MEASUREMENT
            })
            .or_else(|| {
                characteristics.iter().find(|value| {
                    value.service_uuid == FITNESS_MACHINE_SERVICE && value.uuid == INDOOR_BIKE_DATA
                })
            }),
        DeviceRole::HeartRate => characteristics.iter().find(|value| {
            value.service_uuid == HEART_RATE_SERVICE && value.uuid == HEART_RATE_MEASUREMENT
        }),
        DeviceRole::Cadence => characteristics
            .iter()
            .find(|value| {
                value.service_uuid == CYCLING_SPEED_AND_CADENCE_SERVICE
                    && value.uuid == CSC_MEASUREMENT
            })
            .or_else(|| {
                characteristics.iter().find(|value| {
                    value.service_uuid == CYCLING_POWER_SERVICE
                        && value.uuid == CYCLING_POWER_MEASUREMENT
                })
            })
            .or_else(|| {
                characteristics.iter().find(|value| {
                    value.service_uuid == FITNESS_MACHINE_SERVICE && value.uuid == INDOOR_BIKE_DATA
                })
            }),
        DeviceRole::SmartTrainer => None,
    };

    if role == DeviceRole::SmartTrainer {
        let has_ftms = peripheral
            .services()
            .iter()
            .any(|service| service.uuid == FITNESS_MACHINE_SERVICE);
        if !has_ftms {
            disconnect_if_owned(&peripheral, was_connected).await;
            return Err(DeviceError::ConnectionFailed(
                "Fitness Machine Service was not found".to_owned(),
            ));
        }
    } else if characteristic.is_none() {
        disconnect_if_owned(&peripheral, was_connected).await;
        return Err(DeviceError::ConnectionFailed(format!(
            "{} measurement characteristic was not found",
            role_name(role)
        )));
    }

    let notifications = if let Some(characteristic) = characteristic {
        if let Err(error) = peripheral.subscribe(characteristic).await {
            disconnect_if_owned(&peripheral, was_connected).await;
            return Err(connection_failed(error));
        }

        match peripheral.notifications().await {
            Ok(notifications) => Some(notifications),
            Err(error) => {
                disconnect_if_owned(&peripheral, was_connected).await;
                return Err(connection_failed(error));
            }
        }
    } else {
        None
    };

    Ok(DeviceConnection {
        role,
        device_id,
        peripheral,
        notifications,
        cadence_tracker: CadenceTracker::default(),
    })
}

async fn disconnect_if_owned(peripheral: &Peripheral, was_connected: bool) {
    if !was_connected {
        let _ = peripheral.disconnect().await;
    }
}

fn connection_failed(error: impl std::fmt::Display) -> DeviceError {
    DeviceError::ConnectionFailed(error.to_string())
}

fn role_name(role: DeviceRole) -> &'static str {
    match role {
        DeviceRole::PowerSource => "power",
        DeviceRole::SecondaryPowerSource => "secondary power",
        DeviceRole::HeartRate => "heart rate",
        DeviceRole::Cadence => "cadence",
        DeviceRole::SmartTrainer => "smart trainer",
    }
}
