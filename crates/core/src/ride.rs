#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RideSample {
    pub elapsed_secs: u64,
    pub power_watts: u16,
    pub cadence_rpm: u16,
    pub heart_rate_bpm: Option<u8>,
}

impl RideSample {
    pub fn new(
        elapsed_secs: u64,
        power_watts: u16,
        cadence_rpm: u16,
        heart_rate_bpm: Option<u8>,
    ) -> Self {
        Self {
            elapsed_secs,
            power_watts,
            cadence_rpm,
            heart_rate_bpm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RideStats {
    pub elapsed_secs: u64,
    pub average_power_watts: f32,
    pub max_power_watts: u16,
    pub average_cadence_rpm: f32,
    pub max_cadence_rpm: u16,
    pub average_heart_rate_bpm: Option<f32>,
}

impl Default for RideStats {
    fn default() -> Self {
        Self {
            elapsed_secs: 0,
            average_power_watts: 0.0,
            max_power_watts: 0,
            average_cadence_rpm: 0.0,
            max_cadence_rpm: 0,
            average_heart_rate_bpm: None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct RideRecorder {
    samples: Vec<RideSample>,
}

impl RideRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, sample: RideSample) {
        self.samples.push(sample);
    }

    pub fn samples(&self) -> &[RideSample] {
        &self.samples
    }

    pub fn latest(&self) -> Option<&RideSample> {
        self.samples.last()
    }

    pub fn stats(&self) -> RideStats {
        if self.samples.is_empty() {
            return RideStats::default();
        }

        let elapsed_secs = self
            .samples
            .last()
            .map(|sample| sample.elapsed_secs)
            .unwrap_or_default();
        let total_power: u32 = self
            .samples
            .iter()
            .map(|sample| u32::from(sample.power_watts))
            .sum();
        let total_cadence: u32 = self
            .samples
            .iter()
            .map(|sample| u32::from(sample.cadence_rpm))
            .sum();
        let max_power_watts = self
            .samples
            .iter()
            .map(|sample| sample.power_watts)
            .max()
            .unwrap_or_default();
        let max_cadence_rpm = self
            .samples
            .iter()
            .map(|sample| sample.cadence_rpm)
            .max()
            .unwrap_or_default();

        let heart_rate_values: Vec<u8> = self
            .samples
            .iter()
            .filter_map(|sample| sample.heart_rate_bpm)
            .collect();
        let average_heart_rate_bpm = if heart_rate_values.is_empty() {
            None
        } else {
            let total: u32 = heart_rate_values
                .iter()
                .map(|value| u32::from(*value))
                .sum();
            Some(total as f32 / heart_rate_values.len() as f32)
        };

        RideStats {
            elapsed_secs,
            average_power_watts: total_power as f32 / self.samples.len() as f32,
            max_power_watts,
            average_cadence_rpm: total_cadence as f32 / self.samples.len() as f32,
            max_cadence_rpm,
            average_heart_rate_bpm,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_ride_stats() {
        let mut recorder = RideRecorder::new();
        recorder.push(RideSample::new(1, 100, 80, Some(120)));
        recorder.push(RideSample::new(2, 200, 90, Some(130)));
        recorder.push(RideSample::new(3, 150, 85, None));

        let stats = recorder.stats();

        assert_eq!(stats.elapsed_secs, 3);
        assert_eq!(stats.max_power_watts, 200);
        assert_eq!(stats.max_cadence_rpm, 90);
        assert_eq!(stats.average_power_watts, 150.0);
        assert_eq!(stats.average_cadence_rpm, 85.0);
        assert_eq!(stats.average_heart_rate_bpm, Some(125.0));
    }
}
