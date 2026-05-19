use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use indoor_cycling_core::{RideSample, RideStats};

#[derive(Debug, Clone)]
pub struct RideFileStore {
    root: PathBuf,
}

impl RideFileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn save_csv(
        &self,
        ride_id: &str,
        samples: &[RideSample],
        stats: RideStats,
    ) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.root)?;

        let path = self.root.join(format!("{ride_id}.csv"));
        let mut file = File::create(&path)?;

        writeln!(file, "# elapsed_secs,average_power_watts,max_power_watts,average_cadence_rpm,max_cadence_rpm,average_heart_rate_bpm")?;
        writeln!(
            file,
            "# {},{:.1},{},{:.1},{},{}",
            stats.elapsed_secs,
            stats.average_power_watts,
            stats.max_power_watts,
            stats.average_cadence_rpm,
            stats.max_cadence_rpm,
            stats
                .average_heart_rate_bpm
                .map(|value| format!("{value:.1}"))
                .unwrap_or_default()
        )?;
        writeln!(file, "elapsed_secs,power_watts,cadence_rpm,heart_rate_bpm")?;

        for sample in samples {
            writeln!(
                file,
                "{},{},{},{}",
                sample.elapsed_secs,
                sample.power_watts,
                sample.cadence_rpm,
                sample
                    .heart_rate_bpm
                    .map(|value| value.to_string())
                    .unwrap_or_default()
            )?;
        }

        Ok(path)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
