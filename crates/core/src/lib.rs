pub mod ride;
pub mod workout;

pub use ride::{RideRecorder, RideSample, RideStats};
pub use workout::{
    EffortDuration, Workout, WorkoutDifficulty, WorkoutEngine, WorkoutParameters, WorkoutPhase,
    WorkoutStep,
};
