use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceRole {
    PowerSource,
    SecondaryPowerSource,
    HeartRate,
    Cadence,
    SmartTrainer,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceCapabilities {
    pub power: bool,
    pub heart_rate: bool,
    pub cadence: bool,
    pub smart_trainer: bool,
}

impl DeviceCapabilities {
    pub fn supports(self, role: DeviceRole) -> bool {
        match role {
            DeviceRole::PowerSource | DeviceRole::SecondaryPowerSource => self.power,
            DeviceRole::HeartRate => self.heart_rate,
            DeviceRole::Cadence => self.cadence,
            DeviceRole::SmartTrainer => self.smart_trainer,
        }
    }

    pub fn is_empty(self) -> bool {
        !self.power && !self.heart_rate && !self.cadence && !self.smart_trainer
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub id: DeviceId,
    pub name: String,
    pub capabilities: DeviceCapabilities,
    pub signal_strength: Option<i16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryCommand {
    Scan {
        duration: Duration,
    },
    Connect {
        role: DeviceRole,
        device_id: DeviceId,
    },
    Disconnect {
        role: DeviceRole,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceMeasurement {
    PowerWatts(i16),
    HeartRateBpm(u16),
    CadenceRpm(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    ScanStarted,
    DeviceDiscovered(DiscoveredDevice),
    ScanFinished,
    RoleConnecting {
        role: DeviceRole,
        device_id: DeviceId,
    },
    RoleConnected {
        role: DeviceRole,
        device_id: DeviceId,
    },
    Measurement {
        role: DeviceRole,
        measurement: DeviceMeasurement,
    },
    RoleDisconnected {
        role: DeviceRole,
        device_id: DeviceId,
    },
    RoleError {
        role: DeviceRole,
        message: String,
    },
    Error(String),
}
