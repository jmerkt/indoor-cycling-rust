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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Trainer,
    HeartRateMonitor,
    TrainerAndHeartRateMonitor,
}

impl DeviceKind {
    pub fn is_trainer(self) -> bool {
        matches!(self, Self::Trainer | Self::TrainerAndHeartRateMonitor)
    }

    pub fn is_heart_rate_monitor(self) -> bool {
        matches!(
            self,
            Self::HeartRateMonitor | Self::TrainerAndHeartRateMonitor
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    pub signal_strength: Option<i16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryCommand {
    Scan { duration: Duration },
    ConnectHeartRate { device_id: DeviceId },
    DisconnectHeartRate,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    ScanStarted,
    DeviceDiscovered(DiscoveredDevice),
    ScanFinished,
    HeartRateConnecting(DeviceId),
    HeartRateConnected(DeviceId),
    HeartRateMeasurement(u16),
    HeartRateDisconnected(DeviceId),
    HeartRateError(String),
    Error(String),
}
