use indoor_cycling_core::RideSample;

pub trait TrainerDevice {
    fn connect(&mut self) -> Result<(), DeviceError>;
    fn next_sample(&mut self) -> Result<RideSample, DeviceError>;
    fn set_target_power(&mut self, watts: u16) -> Result<(), DeviceError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceError {
    NotConnected,
    ConnectionFailed(String),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConnected => write!(formatter, "trainer is not connected"),
            Self::ConnectionFailed(message) => write!(formatter, "connection failed: {message}"),
        }
    }
}

impl std::error::Error for DeviceError {}

#[derive(Debug, Clone)]
pub struct SimulatedTrainer {
    connected: bool,
    elapsed_secs: u64,
    target_power_watts: u16,
}

impl SimulatedTrainer {
    pub fn new() -> Self {
        Self {
            connected: false,
            elapsed_secs: 0,
            target_power_watts: 120,
        }
    }
}

impl Default for SimulatedTrainer {
    fn default() -> Self {
        Self::new()
    }
}

impl TrainerDevice for SimulatedTrainer {
    fn connect(&mut self) -> Result<(), DeviceError> {
        self.connected = true;
        Ok(())
    }

    fn next_sample(&mut self) -> Result<RideSample, DeviceError> {
        if !self.connected {
            return Err(DeviceError::NotConnected);
        }

        self.elapsed_secs += 1;
        let wave = ((self.elapsed_secs % 20) as i32 - 10).unsigned_abs() as u16;
        let power_watts = self.target_power_watts.saturating_sub(5) + wave;
        let cadence_rpm = 82 + (self.elapsed_secs % 8) as u16;
        let heart_rate_bpm = Some((118 + (self.elapsed_secs / 10).min(35)) as u8);

        Ok(RideSample::new(
            self.elapsed_secs,
            power_watts,
            cadence_rpm,
            heart_rate_bpm,
        ))
    }

    fn set_target_power(&mut self, watts: u16) -> Result<(), DeviceError> {
        self.target_power_watts = watts;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_trainer_requires_connection() {
        let mut trainer = SimulatedTrainer::new();

        assert_eq!(trainer.next_sample(), Err(DeviceError::NotConnected));
    }

    #[test]
    fn simulated_trainer_produces_samples() {
        let mut trainer = SimulatedTrainer::new();
        trainer.connect().unwrap();
        trainer.set_target_power(180).unwrap();

        let sample = trainer.next_sample().unwrap();

        assert_eq!(sample.elapsed_secs, 1);
        assert!(sample.power_watts > 0);
        assert!(sample.cadence_rpm > 0);
    }
}
