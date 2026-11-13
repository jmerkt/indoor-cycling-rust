use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use indoor_cycling_core::{RideRecorder, RideSample, RideStats};
use serde::{Deserialize, Serialize};

const APP_DIRECTORY: &str = "indoor-cycling";
const CONFIG_FILE: &str = "settings.json";
const RIDES_DIRECTORY: &str = "rides";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkoutBuilderConfiguration {
    pub length_minutes: i32,
    #[serde(default = "default_workout_type")]
    pub workout_type: i32,
    #[serde(alias = "difficulty", default = "default_builder_level")]
    pub effort_intensity: i32,
    #[serde(default = "default_builder_level")]
    pub recovery_intensity: i32,
    pub effort_duration: i32,
    #[serde(default = "default_builder_level")]
    pub recovery_duration: i32,
    pub variation: i32,
}

fn default_builder_level() -> i32 {
    2
}

fn default_workout_type() -> i32 {
    3
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
    #[serde(default)]
    pub compact_dashboard: bool,
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
    pub fn for_current_user() -> io::Result<Self> {
        Ok(Self {
            root: data_directory()?.join(RIDES_DIRECTORY),
        })
    }

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

    pub fn save_fit(&self, ride_id: &str, samples: &[RideSample]) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.root)?;

        let path = self.root.join(format!("{ride_id}.fit"));
        let finish_time = SystemTime::now();
        let contents = fit::activity_file(samples, finish_time)?;
        fs::write(&path, contents)?;

        Ok(path)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn data_directory() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from);

    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"));

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));

    root.map(|root| root.join(APP_DIRECTORY)).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine a per-user data directory",
        )
    })
}

mod fit {
    use super::*;

    const GARMIN_EPOCH_UNIX_SECS: u64 = 631_065_600;
    const FIT_PROTOCOL_VERSION: u8 = 16;
    const FIT_PROFILE_VERSION: u16 = 2_130;

    const TYPE_ENUM: u8 = 0x00;
    const TYPE_UINT8: u8 = 0x02;
    const TYPE_UINT16: u8 = 0x84;
    const TYPE_UINT32: u8 = 0x86;

    const MSG_FILE_ID: u16 = 0;
    const MSG_SESSION: u16 = 18;
    const MSG_LAP: u16 = 19;
    const MSG_RECORD: u16 = 20;
    const MSG_ACTIVITY: u16 = 34;

    const FILE_ACTIVITY: u8 = 4;
    const MANUFACTURER_DEVELOPMENT: u16 = 255;
    const SPORT_CYCLING: u8 = 2;
    const SUB_SPORT_INDOOR_CYCLING: u8 = 6;
    const EVENT_TIMER: u8 = 0;
    const EVENT_TYPE_STOP: u8 = 1;
    const ACTIVITY_MANUAL: u8 = 0;
    const SESSION_FIELDS: [Field; 20] = [
        Field::new(253, 4, TYPE_UINT32),
        Field::new(0, 1, TYPE_ENUM),
        Field::new(1, 1, TYPE_ENUM),
        Field::new(2, 4, TYPE_UINT32),
        Field::new(5, 1, TYPE_ENUM),
        Field::new(6, 1, TYPE_ENUM),
        Field::new(7, 4, TYPE_UINT32),
        Field::new(8, 4, TYPE_UINT32),
        Field::new(9, 4, TYPE_UINT32),
        Field::new(16, 1, TYPE_UINT8),
        Field::new(17, 1, TYPE_UINT8),
        Field::new(18, 1, TYPE_UINT8),
        Field::new(19, 1, TYPE_UINT8),
        Field::new(20, 2, TYPE_UINT16),
        Field::new(21, 2, TYPE_UINT16),
        Field::new(22, 2, TYPE_UINT16),
        Field::new(23, 2, TYPE_UINT16),
        Field::new(25, 2, TYPE_UINT16),
        Field::new(26, 2, TYPE_UINT16),
        Field::new(48, 4, TYPE_UINT32),
    ];
    const LAP_FIELDS: [Field; 19] = [
        Field::new(253, 4, TYPE_UINT32),
        Field::new(0, 1, TYPE_ENUM),
        Field::new(1, 1, TYPE_ENUM),
        Field::new(2, 4, TYPE_UINT32),
        Field::new(7, 4, TYPE_UINT32),
        Field::new(8, 4, TYPE_UINT32),
        Field::new(9, 4, TYPE_UINT32),
        Field::new(15, 1, TYPE_UINT8),
        Field::new(16, 1, TYPE_UINT8),
        Field::new(17, 1, TYPE_UINT8),
        Field::new(18, 1, TYPE_UINT8),
        Field::new(19, 2, TYPE_UINT16),
        Field::new(20, 2, TYPE_UINT16),
        Field::new(21, 2, TYPE_UINT16),
        Field::new(22, 2, TYPE_UINT16),
        Field::new(23, 1, TYPE_ENUM),
        Field::new(25, 1, TYPE_ENUM),
        Field::new(39, 1, TYPE_ENUM),
        Field::new(41, 4, TYPE_UINT32),
    ];
    const ZERO_DISTANCE: u32 = 0;
    const ZERO_ASCENT_METERS: u16 = 0;
    const ZERO_DESCENT_METERS: u16 = 0;
    const ZERO_INTENSITY: u8 = 0;
    const FIRST_LAP_INDEX: u16 = 0;
    const SINGLE_LAP: u16 = 1;

    pub fn activity_file(samples: &[RideSample], finish_time: SystemTime) -> io::Result<Vec<u8>> {
        if samples.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot save a FIT file without ride samples",
            ));
        }

        let stats = RideRecorder::from_samples(samples).stats();
        let elapsed_secs = stats.elapsed_secs.max(1);
        let finish_unix_secs = finish_time
            .duration_since(UNIX_EPOCH)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
            .as_secs();
        let start_unix_secs = finish_unix_secs.saturating_sub(elapsed_secs);
        let start_timestamp = fit_timestamp(start_unix_secs)?;
        let finish_timestamp = fit_timestamp(finish_unix_secs)?;
        let total_timer_time = saturating_millis(elapsed_secs);
        let total_work = total_work_joules(samples);

        let mut data = FitData::new();
        data.define(
            0,
            MSG_FILE_ID,
            &[
                Field::new(3, 4, TYPE_UINT32),
                Field::new(4, 4, TYPE_UINT32),
                Field::new(1, 2, TYPE_UINT16),
                Field::new(2, 2, TYPE_UINT16),
                Field::new(0, 1, TYPE_ENUM),
            ],
        );
        data.file_id(start_timestamp);

        data.define(
            1,
            MSG_RECORD,
            &[
                Field::new(253, 4, TYPE_UINT32),
                Field::new(7, 2, TYPE_UINT16),
                Field::new(4, 1, TYPE_UINT8),
                Field::new(3, 1, TYPE_UINT8),
            ],
        );
        for sample in samples {
            data.record(start_timestamp, sample);
        }

        let max_heart_rate_bpm = samples
            .iter()
            .filter_map(|sample| sample.heart_rate_bpm)
            .max()
            .unwrap_or(0xFF);
        let summary = RideSummary::new(stats, total_timer_time, total_work, max_heart_rate_bpm);
        data.define(2, MSG_LAP, &LAP_FIELDS);
        data.lap(start_timestamp, finish_timestamp, &summary);

        data.define(3, MSG_SESSION, &SESSION_FIELDS);
        data.session(start_timestamp, finish_timestamp, &summary);

        data.define(
            4,
            MSG_ACTIVITY,
            &[
                Field::new(253, 4, TYPE_UINT32),
                Field::new(0, 4, TYPE_UINT32),
                Field::new(1, 2, TYPE_UINT16),
                Field::new(2, 1, TYPE_ENUM),
                Field::new(3, 1, TYPE_ENUM),
                Field::new(4, 1, TYPE_ENUM),
            ],
        );
        data.activity(finish_timestamp, total_timer_time);

        Ok(with_fit_header(data.into_bytes()))
    }

    #[derive(Clone, Copy)]
    struct Field {
        number: u8,
        size: u8,
        base_type: u8,
    }

    impl Field {
        const fn new(number: u8, size: u8, base_type: u8) -> Self {
            Self {
                number,
                size,
                base_type,
            }
        }
    }

    struct RideSummary {
        total_timer_time: u32,
        total_work: u32,
        average_power_watts: u16,
        max_power_watts: u16,
        average_cadence_rpm: u8,
        max_cadence_rpm: u8,
        average_heart_rate_bpm: u8,
        max_heart_rate_bpm: u8,
    }

    impl RideSummary {
        fn new(
            stats: indoor_cycling_core::RideStats,
            total_timer_time: u32,
            total_work: u32,
            max_heart_rate_bpm: u8,
        ) -> Self {
            Self {
                total_timer_time,
                total_work,
                average_power_watts: stats.average_power_watts.round().clamp(0., u16::MAX as f32)
                    as u16,
                max_power_watts: stats.max_power_watts,
                average_cadence_rpm: stats.average_cadence_rpm.round().clamp(0., u8::MAX as f32)
                    as u8,
                max_cadence_rpm: stats.max_cadence_rpm.min(u16::from(u8::MAX)) as u8,
                average_heart_rate_bpm: stats
                    .average_heart_rate_bpm
                    .map(|value| value.round().clamp(0., 254.) as u8)
                    .unwrap_or(0xFF),
                max_heart_rate_bpm,
            }
        }
    }

    struct FitData {
        bytes: Vec<u8>,
    }

    impl FitData {
        fn new() -> Self {
            Self { bytes: Vec::new() }
        }

        fn define(&mut self, local_message: u8, global_message: u16, fields: &[Field]) {
            self.bytes.push(0x40 | local_message);
            self.bytes.push(0);
            self.bytes.push(0);
            self.write_u16(global_message);
            self.bytes.push(fields.len().min(u8::MAX as usize) as u8);
            for field in fields {
                self.bytes.push(field.number);
                self.bytes.push(field.size);
                self.bytes.push(field.base_type);
            }
        }

        fn file_id(&mut self, time_created: u32) {
            self.bytes.push(0);
            self.write_u32(u32::from(std::process::id()));
            self.write_u32(time_created);
            self.write_u16(MANUFACTURER_DEVELOPMENT);
            self.write_u16(1);
            self.bytes.push(FILE_ACTIVITY);
        }

        fn record(&mut self, start_timestamp: u32, sample: &RideSample) {
            self.bytes.push(1);
            self.write_u32(
                start_timestamp.saturating_add(sample.elapsed_secs.min(u64::from(u32::MAX)) as u32),
            );
            self.write_u16(sample.power_watts);
            self.bytes
                .push(sample.cadence_rpm.min(u16::from(u8::MAX)) as u8);
            self.bytes.push(sample.heart_rate_bpm.unwrap_or(0xFF));
        }

        fn lap(&mut self, start_time: u32, timestamp: u32, summary: &RideSummary) {
            self.bytes.push(2);
            self.write_u32(timestamp);
            self.bytes.push(EVENT_TIMER);
            self.bytes.push(EVENT_TYPE_STOP);
            self.write_u32(start_time);
            self.write_u32(summary.total_timer_time);
            self.write_u32(summary.total_timer_time);
            self.write_u32(ZERO_DISTANCE);
            self.bytes.push(summary.average_heart_rate_bpm);
            self.bytes.push(summary.max_heart_rate_bpm);
            self.bytes.push(summary.average_cadence_rpm);
            self.bytes.push(summary.max_cadence_rpm);
            self.write_u16(summary.average_power_watts);
            self.write_u16(summary.max_power_watts);
            self.write_u16(ZERO_ASCENT_METERS);
            self.write_u16(ZERO_DESCENT_METERS);
            self.bytes.push(ZERO_INTENSITY);
            self.bytes.push(SPORT_CYCLING);
            self.bytes.push(SUB_SPORT_INDOOR_CYCLING);
            self.write_u32(summary.total_work);
        }

        fn session(&mut self, start_time: u32, timestamp: u32, summary: &RideSummary) {
            self.bytes.push(3);
            self.write_u32(timestamp);
            self.bytes.push(EVENT_TIMER);
            self.bytes.push(EVENT_TYPE_STOP);
            self.write_u32(start_time);
            self.bytes.push(SPORT_CYCLING);
            self.bytes.push(SUB_SPORT_INDOOR_CYCLING);
            self.write_u32(summary.total_timer_time);
            self.write_u32(summary.total_timer_time);
            self.write_u32(ZERO_DISTANCE);
            self.bytes.push(summary.average_heart_rate_bpm);
            self.bytes.push(summary.max_heart_rate_bpm);
            self.bytes.push(summary.average_cadence_rpm);
            self.bytes.push(summary.max_cadence_rpm);
            self.write_u16(summary.average_power_watts);
            self.write_u16(summary.max_power_watts);
            self.write_u16(ZERO_ASCENT_METERS);
            self.write_u16(ZERO_DESCENT_METERS);
            self.write_u16(FIRST_LAP_INDEX);
            self.write_u16(SINGLE_LAP);
            self.write_u32(summary.total_work);
        }

        fn activity(&mut self, timestamp: u32, total_timer_time: u32) {
            self.bytes.push(4);
            self.write_u32(timestamp);
            self.write_u32(total_timer_time);
            self.write_u16(1);
            self.bytes.push(ACTIVITY_MANUAL);
            self.bytes.push(EVENT_TIMER);
            self.bytes.push(EVENT_TYPE_STOP);
        }

        fn write_u16(&mut self, value: u16) {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }

        fn write_u32(&mut self, value: u32) {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }

        fn into_bytes(self) -> Vec<u8> {
            self.bytes
        }
    }

    trait RideRecorderExt {
        fn from_samples(samples: &[RideSample]) -> RideRecorder;
    }

    impl RideRecorderExt for RideRecorder {
        fn from_samples(samples: &[RideSample]) -> RideRecorder {
            let mut recorder = RideRecorder::new();
            for sample in samples {
                recorder.push(sample.clone());
            }
            recorder
        }
    }

    fn fit_timestamp(unix_secs: u64) -> io::Result<u32> {
        unix_secs
            .checked_sub(GARMIN_EPOCH_UNIX_SECS)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "ride timestamp is outside the FIT timestamp range",
                )
            })
    }

    fn saturating_millis(seconds: u64) -> u32 {
        seconds.saturating_mul(1_000).min(u64::from(u32::MAX)) as u32
    }

    fn total_work_joules(samples: &[RideSample]) -> u32 {
        samples
            .iter()
            .map(|sample| u32::from(sample.power_watts))
            .sum()
    }

    fn with_fit_header(data: Vec<u8>) -> Vec<u8> {
        let mut file = Vec::with_capacity(14 + data.len() + 2);
        file.push(14);
        file.push(FIT_PROTOCOL_VERSION);
        file.extend_from_slice(&FIT_PROFILE_VERSION.to_le_bytes());
        file.extend_from_slice(&(data.len().min(u32::MAX as usize) as u32).to_le_bytes());
        file.extend_from_slice(b".FIT");
        let header_crc = crc(&file);
        file.extend_from_slice(&header_crc.to_le_bytes());
        file.extend_from_slice(&data);
        let file_crc = crc(&file);
        file.extend_from_slice(&file_crc.to_le_bytes());
        file
    }

    fn crc(bytes: &[u8]) -> u16 {
        const TABLE: [u16; 16] = [
            0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
            0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
        ];

        let mut crc = 0;
        for byte in bytes {
            let mut tmp = TABLE[(crc & 0xF) as usize ^ usize::from(byte & 0xF)];
            crc = (crc >> 4) ^ tmp;
            tmp = TABLE[(crc & 0xF) as usize ^ usize::from(byte >> 4)];
            crc = (crc >> 4) ^ tmp;
        }
        crc
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::Duration;

        #[test]
        fn writes_activity_fit_file_with_records_and_crc() {
            let samples = vec![
                RideSample::new(1, 120, 84, Some(140)),
                RideSample::new(2, 130, 86, None),
            ];

            let file =
                activity_file(&samples, UNIX_EPOCH + Duration::from_secs(1_700_000_000)).unwrap();

            assert_eq!(file[0], 14);
            assert_eq!(&file[8..12], b".FIT");
            let data_size = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
            assert_eq!(file.len(), 14 + data_size + 2);
            assert_eq!(file[14], 0x40);
            assert_eq!(file[15], 0);
            assert_eq!(file[16], 0);
            assert_eq!(
                u16::from_le_bytes(file[17..19].try_into().unwrap()),
                MSG_FILE_ID
            );
            assert_eq!(file[19], 5);
            let definitions = definitions(&file);
            assert_eq!(
                definitions.get(&MSG_LAP).unwrap(),
                &vec![253, 0, 1, 2, 7, 8, 9, 15, 16, 17, 18, 19, 20, 21, 22, 23, 25, 39, 41]
            );
            assert_eq!(
                definitions.get(&MSG_SESSION).unwrap(),
                &vec![253, 0, 1, 2, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 25, 26, 48]
            );
            assert!(!definitions.get(&MSG_LAP).unwrap().contains(&48));
            assert!(
                file[14..14 + data_size]
                    .iter()
                    .filter(|byte| **byte == 1)
                    .count()
                    >= 2
            );
            let stored_crc = u16::from_le_bytes(file[file.len() - 2..].try_into().unwrap());
            assert_eq!(stored_crc, crc(&file[..file.len() - 2]));
        }

        #[test]
        fn rejects_empty_rides() {
            let error =
                activity_file(&[], UNIX_EPOCH + Duration::from_secs(1_700_000_000)).unwrap_err();

            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }

        fn definitions(file: &[u8]) -> std::collections::HashMap<u16, Vec<u8>> {
            let data_size = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
            let data = &file[14..14 + data_size];
            let mut offset = 0;
            let mut local_sizes = [0usize; 16];
            let mut definitions = std::collections::HashMap::new();

            while offset < data.len() {
                let header = data[offset];
                offset += 1;
                let local_message = usize::from(header & 0x0F);
                if header & 0x40 != 0 {
                    offset += 1;
                    let architecture = data[offset];
                    offset += 1;
                    assert_eq!(architecture, 0);
                    let global_message =
                        u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap());
                    offset += 2;
                    let field_count = usize::from(data[offset]);
                    offset += 1;
                    let mut field_numbers = Vec::with_capacity(field_count);
                    let mut record_size = 0usize;
                    for _ in 0..field_count {
                        field_numbers.push(data[offset]);
                        record_size += usize::from(data[offset + 1]);
                        offset += 3;
                    }
                    local_sizes[local_message] = record_size;
                    definitions.insert(global_message, field_numbers);
                } else {
                    offset += local_sizes[local_message];
                }
            }

            definitions
        }
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
            compact_dashboard: true,
            version: 3,
            ftp_watts: 245,
            workout_builder: WorkoutBuilderConfiguration {
                length_minutes: 60,
                workout_type: 4,
                effort_intensity: 9,
                recovery_intensity: 6,
                effort_duration: 4,
                recovery_duration: 10,
                variation: 9,
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
                logical_height: 340.,
            }),
        };

        store.save(&configuration).unwrap();
        assert_eq!(store.load().unwrap(), Some(configuration));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_workout_builder_configuration_loads() {
        let path = std::env::temp_dir().join(format!(
            "indoor-cycling-legacy-settings-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ConfigurationStore::new(&path);
        fs::write(
            &path,
            r#"{
  "version": 1,
  "ftp_watts": 245,
  "workout_builder": {
    "length_minutes": 60,
    "difficulty": 3,
    "effort_duration": 1,
    "variation": 4
  },
  "latest_workout": {
    "name": "Hard Short Intervals",
    "steps": []
  },
  "window": null
}"#,
        )
        .unwrap();

        let configuration = store.load().unwrap().unwrap();
        assert!(!configuration.compact_dashboard);
        assert_eq!(configuration.workout_builder.effort_intensity, 3);
        assert_eq!(configuration.workout_builder.workout_type, 3);
        assert_eq!(configuration.workout_builder.recovery_intensity, 2);
        assert_eq!(configuration.workout_builder.effort_duration, 1);
        assert_eq!(configuration.workout_builder.recovery_duration, 2);
        fs::remove_file(path).unwrap();
    }
}
