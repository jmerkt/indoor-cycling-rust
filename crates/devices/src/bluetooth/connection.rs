use std::future::Future;
use std::pin::Pin;

use btleplug::api::{Characteristic, Peripheral as _, ValueNotification, WriteType};
use btleplug::platform::Peripheral;
use futures::{Stream, StreamExt};
use tokio::time::{timeout, Duration};

use crate::bluetooth::cadence::{self, CadenceTracker};
use crate::bluetooth::cycling_power;
use crate::bluetooth::fitness_machine;
use crate::bluetooth::heart_rate;
use crate::bluetooth::uuids::{
    CSC_MEASUREMENT, CYCLING_POWER_MEASUREMENT, CYCLING_POWER_SERVICE,
    CYCLING_SPEED_AND_CADENCE_SERVICE, FITNESS_MACHINE_CONTROL_POINT, FITNESS_MACHINE_SERVICE,
    HEART_RATE_MEASUREMENT, HEART_RATE_SERVICE, INDOOR_BIKE_DATA, SUPPORTED_RESISTANCE_LEVEL_RANGE,
};
use crate::{DeviceError, DeviceId, DeviceMeasurement, DeviceRole};

const DEFAULT_MANUAL_RESISTANCE_LEVEL: i16 = 200;

pub struct DeviceConnection {
    pub role: DeviceRole,
    pub device_id: DeviceId,
    pub peripheral: Peripheral,
    pub notifications: Option<Pin<Box<dyn Stream<Item = ValueNotification> + Send>>>,
    control_point: Option<Characteristic>,
    control_acquired: bool,
    erg_active: bool,
    manual_resistance_level: Option<i16>,
    manual_resistance_fallback: i16,
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
            DeviceRole::SmartTrainer if notification.uuid == INDOOR_BIKE_DATA => {
                let data = fitness_machine::parse_indoor_bike_data(&notification.value)?;
                if !self.erg_active {
                    if let Some(resistance_level) = data.resistance_level {
                        // Some trainers report zero in their uncontrolled state
                        // even though their firmware still provides free-ride load.
                        if resistance_level > 0 {
                            self.manual_resistance_level = Some(resistance_level);
                        }
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    pub async fn set_erg_mode(
        &mut self,
        enabled: bool,
        target_power_watts: u16,
    ) -> Result<(), DeviceError> {
        if enabled {
            if !self.erg_active && self.manual_resistance_level.is_none() {
                self.manual_resistance_level = Some(self.manual_resistance_fallback);
            }
            if !self.control_acquired {
                self.write_control_command(&fitness_machine::request_control_command())
                    .await?;
                self.control_acquired = true;
            }
            self.set_erg_target_power(target_power_watts).await?;
            self.erg_active = true;
            Ok(())
        } else {
            if !self.control_acquired {
                self.erg_active = false;
                return Ok(());
            }
            let manual_resistance = self
                .manual_resistance_level
                .unwrap_or(self.manual_resistance_fallback);
            // Switching to resistance mode reliably clears target-power mode on
            // trainers that accept Stop/Reset without actually clearing ERG.
            if let Err(resistance_error) = self
                .write_control_command(&fitness_machine::set_target_resistance_level_command(
                    manual_resistance,
                ))
                .await
            {
                if let Err(stop_error) = self
                    .write_control_command(&fitness_machine::stop_command())
                    .await
                {
                    self.write_control_command(&fitness_machine::reset_command())
                        .await
                        .map_err(|reset_error| {
                            DeviceError::ControlFailed(format!(
                                "could not leave ERG mode with resistance target ({resistance_error}); stop failed ({stop_error}); reset fallback failed ({reset_error})"
                            ))
                        })?;
                    self.control_acquired = false;
                } else {
                    self.control_acquired = false;
                }
            }
            self.erg_active = false;
            Ok(())
        }
    }

    pub async fn set_erg_target_power(
        &mut self,
        target_power_watts: u16,
    ) -> Result<(), DeviceError> {
        if !self.control_acquired {
            return Err(DeviceError::ControlFailed(
                "ERG mode is not enabled".to_owned(),
            ));
        }

        self.write_control_command(&fitness_machine::set_target_power_command(
            target_power_watts,
        ))
        .await
    }

    async fn write_control_command(&mut self, command: &[u8]) -> Result<(), DeviceError> {
        let characteristic = self.control_point.as_ref().ok_or_else(|| {
            DeviceError::ControlFailed("Fitness Machine Control Point was not found".to_owned())
        })?;
        let expected_opcode = command[0];

        self.peripheral
            .write(characteristic, command, WriteType::WithResponse)
            .await
            .map_err(control_failed)?;

        let notifications = self.notifications.as_mut().ok_or_else(|| {
            DeviceError::ControlFailed(
                "Fitness Machine Control Point indications are unavailable".to_owned(),
            )
        })?;
        let response = timeout(Duration::from_secs(5), async {
            while let Some(notification) = notifications.next().await {
                if notification.uuid == FITNESS_MACHINE_CONTROL_POINT {
                    return Some(notification);
                }
            }
            None
        })
        .await
        .map_err(|_| DeviceError::ControlFailed("trainer response timed out".to_owned()))?
        .ok_or_else(|| DeviceError::ControlFailed("trainer disconnected".to_owned()))?;

        fitness_machine::parse_control_point_response(&response.value, expected_opcode)
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
    let control_point = characteristics
        .iter()
        .find(|value| {
            value.service_uuid == FITNESS_MACHINE_SERVICE
                && value.uuid == FITNESS_MACHINE_CONTROL_POINT
        })
        .cloned();
    let indoor_bike_data = characteristics
        .iter()
        .find(|value| {
            value.service_uuid == FITNESS_MACHINE_SERVICE && value.uuid == INDOOR_BIKE_DATA
        })
        .cloned();
    let supported_resistance_level_range = characteristics.iter().find(|value| {
        value.service_uuid == FITNESS_MACHINE_SERVICE
            && value.uuid == SUPPORTED_RESISTANCE_LEVEL_RANGE
    });

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
        if control_point.is_none() {
            disconnect_if_owned(&peripheral, was_connected).await;
            return Err(DeviceError::ConnectionFailed(
                "Fitness Machine Control Point was not found".to_owned(),
            ));
        }
    } else if characteristic.is_none() {
        disconnect_if_owned(&peripheral, was_connected).await;
        return Err(DeviceError::ConnectionFailed(format!(
            "{} measurement characteristic was not found",
            role_name(role)
        )));
    }

    let mut subscribed_characteristics = Vec::new();
    if role == DeviceRole::SmartTrainer {
        if let Some(control_point) = control_point.as_ref() {
            subscribed_characteristics.push(control_point);
        }
        if let Some(indoor_bike_data) = indoor_bike_data.as_ref() {
            subscribed_characteristics.push(indoor_bike_data);
        }
    } else if let Some(characteristic) = characteristic {
        subscribed_characteristics.push(characteristic);
    }

    let notifications = if subscribed_characteristics.is_empty() {
        None
    } else {
        for subscribed_characteristic in subscribed_characteristics {
            if let Err(error) = peripheral.subscribe(subscribed_characteristic).await {
                disconnect_if_owned(&peripheral, was_connected).await;
                return Err(connection_failed(error));
            }
        }

        match peripheral.notifications().await {
            Ok(notifications) => {
                // On BlueZ, the notification stream stays open after a physical
                // disconnect. End it explicitly so the service clears every
                // affected role instead of displaying the last sample forever.
                let monitored_peripheral = peripheral.clone();
                let disconnected = wait_for_disconnect(
                    move || {
                        let peripheral = monitored_peripheral.clone();
                        async move { peripheral.is_connected().await }
                    },
                    Duration::from_secs(1),
                    Duration::from_secs(2),
                );
                Some(Box::pin(notifications.take_until(disconnected))
                    as Pin<Box<dyn Stream<Item = ValueNotification> + Send>>)
            }
            Err(error) => {
                disconnect_if_owned(&peripheral, was_connected).await;
                return Err(connection_failed(error));
            }
        }
    };

    let manual_resistance_fallback = if role == DeviceRole::SmartTrainer {
        if let Some(characteristic) = supported_resistance_level_range {
            peripheral
                .read(characteristic)
                .await
                .ok()
                .and_then(|value| {
                    fitness_machine::parse_supported_resistance_level_range(&value).ok()
                })
                .map(default_manual_resistance_level)
                .unwrap_or(DEFAULT_MANUAL_RESISTANCE_LEVEL)
        } else {
            DEFAULT_MANUAL_RESISTANCE_LEVEL
        }
    } else {
        DEFAULT_MANUAL_RESISTANCE_LEVEL
    };

    Ok(DeviceConnection {
        role,
        device_id,
        peripheral,
        notifications,
        control_point,
        control_acquired: false,
        erg_active: false,
        manual_resistance_level: None,
        manual_resistance_fallback,
        cadence_tracker: CadenceTracker::default(),
    })
}

async fn wait_for_disconnect<Check, CheckFuture, Error>(
    mut check: Check,
    interval: Duration,
    query_timeout: Duration,
) where
    Check: FnMut() -> CheckFuture,
    CheckFuture: Future<Output = Result<bool, Error>>,
{
    loop {
        tokio::time::sleep(interval).await;
        if !matches!(timeout(query_timeout, check()).await, Ok(Ok(true))) {
            return;
        }
    }
}

fn default_manual_resistance_level(range: fitness_machine::SupportedResistanceLevelRange) -> i16 {
    let minimum = i32::from(range.minimum);
    let maximum = i32::from(range.maximum);
    if maximum <= minimum {
        return range.minimum;
    }

    let increment = i32::from(range.minimum_increment.max(1));
    let desired = minimum + (maximum - minimum) / 5;
    let steps = (desired - minimum + increment / 2) / increment;
    (minimum + steps * increment).clamp(minimum, maximum) as i16
}

async fn disconnect_if_owned(peripheral: &Peripheral, was_connected: bool) {
    if !was_connected {
        let _ = peripheral.disconnect().await;
    }
}

fn connection_failed(error: impl std::fmt::Display) -> DeviceError {
    DeviceError::ConnectionFailed(error.to_string())
}

fn control_failed(error: impl std::fmt::Display) -> DeviceError {
    DeviceError::ControlFailed(error.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn notification_stream_ends_when_the_physical_link_is_lost() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let connected = Arc::new(AtomicBool::new(true));
        let check = Arc::clone(&connected);
        let disconnected = wait_for_disconnect(
            move || std::future::ready(Ok::<_, ()>(check.load(Ordering::Relaxed))),
            Duration::from_millis(1),
            Duration::from_millis(20),
        );
        let (sender, stream) = futures::channel::mpsc::unbounded::<u8>();
        let mut notifications = Box::pin(stream.take_until(disconnected));
        sender.unbounded_send(42).unwrap();
        assert_eq!(notifications.next().await, Some(42));
        connected.store(false, Ordering::Relaxed);
        assert_eq!(
            timeout(Duration::from_secs(1), notifications.next())
                .await
                .unwrap(),
            None
        );
        // The source itself never closed: only the link-health check ended it.
        assert!(!sender.is_closed());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_device_ends_health_monitor_after_a_successful_check() {
        let mut checks = [Ok(true), Err(())].into_iter();
        timeout(
            Duration::from_secs(1),
            wait_for_disconnect(
                || std::future::ready(checks.next().expect("health monitor must stop on error")),
                Duration::from_millis(1),
                Duration::from_millis(20),
            ),
        )
        .await
        .unwrap();
        assert!(checks.next().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unresponsive_backend_does_not_leave_health_monitor_waiting_forever() {
        timeout(
            Duration::from_secs(1),
            wait_for_disconnect(
                std::future::pending::<Result<bool, ()>>,
                Duration::from_millis(1),
                Duration::from_millis(10),
            ),
        )
        .await
        .unwrap();
    }

    #[test]
    fn chooses_a_moderate_supported_manual_resistance() {
        assert_eq!(
            default_manual_resistance_level(fitness_machine::SupportedResistanceLevelRange {
                minimum: 0,
                maximum: 1000,
                minimum_increment: 10,
            }),
            200
        );
    }

    #[test]
    fn rounds_manual_resistance_to_a_supported_increment() {
        assert_eq!(
            default_manual_resistance_level(fitness_machine::SupportedResistanceLevelRange {
                minimum: 50,
                maximum: 400,
                minimum_increment: 25,
            }),
            125
        );
    }
}
