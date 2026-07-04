pub mod ride;
pub mod workout;

pub use ride::{RideRecorder, RideSample, RideStats};
pub use workout::{
    EffortDuration, EffortIntensity, RecoveryDuration, RecoveryIntensity, Workout,
    WorkoutDifficulty, WorkoutEngine, WorkoutParameters, WorkoutPhase, WorkoutStep, WorkoutType,
};
