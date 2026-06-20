use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use indoor_cycling_core::{RideSample, RideStats};
use serde::{Deserialize, Serialize};

const APP_DIRECTORY: &str = "indoor-cycling";
const CONFIG_FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkoutBuilderConfiguration {
    pub length_minutes: i32,
    pub difficulty: i32,
    pub effort_duration: i32,
    pub variation: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWorkoutStep {
    pub name: String,
    pub duration_secs: u64,
    pub target_ftp_percent: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWorkout {
    pub name: String,
    pub steps: Vec<StoredWorkoutStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowConfiguration {
    pub logical_width: f32,
    pub logical_height: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfiguration {
    pub version: u8,
    pub ftp_watts: u16,
    pub workout_builder: WorkoutBuilderConfiguration,
    pub latest_workout: StoredWorkout,
    #[serde(default)]
    pub window: Option<WindowConfiguration>,
}

#[derive(Debug, Clone)]
pub struct ConfigurationStore {
    path: PathBuf,
}

impl ConfigurationStore {
    pub fn for_current_user() -> io::Result<Self> {
        Ok(Self {
            path: configuration_directory()?.join(CONFIG_FILE),
        })
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> io::Result<Option<AppConfiguration>> {
        match fs::read(&self.path) {
            Ok(contents) => serde_json::from_slice(&contents)
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, configuration: &AppConfiguration) -> io::Result<()> {
        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration path has no parent",
            )
        })?;
        fs::create_dir_all(parent)?;
        let contents = serde_json::to_vec_pretty(configuration)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(&self.path, contents)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn configuration_directory() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);

    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"));

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));

    root.map(|root| root.join(APP_DIRECTORY)).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine a per-user configuration directory",
        )
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "indoor-cycling-settings-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ConfigurationStore::new(&path);
        let configuration = AppConfiguration {
            version: 1,
            ftp_watts: 245,
            workout_builder: WorkoutBuilderConfiguration {
                length_minutes: 60,
                difficulty: 3,
                effort_duration: 1,
                variation: 4,
            },
            latest_workout: StoredWorkout {
                name: "Hard Short Intervals".to_owned(),
                steps: vec![StoredWorkoutStep {
                    name: "Warmup".to_owned(),
                    duration_secs: 300,
                    target_ftp_percent: 55,
                }],
            },
            window: Some(WindowConfiguration {
                logical_width: 1100.,
                logical_height: 820.,
            }),
        };

        store.save(&configuration).unwrap();
        assert_eq!(store.load().unwrap(), Some(configuration));
        fs::remove_file(path).unwrap();
    }
}
