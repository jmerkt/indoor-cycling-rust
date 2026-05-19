#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkoutStep {
    pub name: String,
    pub duration_secs: u64,
    pub target_power_watts: u16,
}

impl WorkoutStep {
    pub fn new(name: impl Into<String>, duration_secs: u64, target_power_watts: u16) -> Self {
        Self {
            name: name.into(),
            duration_secs,
            target_power_watts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workout {
    pub name: String,
    pub steps: Vec<WorkoutStep>,
}

impl Workout {
    pub fn new(name: impl Into<String>, steps: Vec<WorkoutStep>) -> Self {
        Self {
            name: name.into(),
            steps,
        }
    }

    pub fn endurance_sample() -> Self {
        Self::new(
            "Endurance Starter",
            vec![
                WorkoutStep::new("Warmup", 300, 100),
                WorkoutStep::new("Steady", 900, 150),
                WorkoutStep::new("Tempo", 300, 180),
                WorkoutStep::new("Cooldown", 300, 90),
            ],
        )
    }

    pub fn total_duration_secs(&self) -> u64 {
        self.steps.iter().map(|step| step.duration_secs).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkoutPhase {
    NotStarted,
    Active {
        step_index: usize,
        step_elapsed_secs: u64,
        step: WorkoutStep,
    },
    Finished,
}

#[derive(Debug, Clone)]
pub struct WorkoutEngine {
    workout: Workout,
    elapsed_secs: u64,
    running: bool,
}

impl WorkoutEngine {
    pub fn new(workout: Workout) -> Self {
        Self {
            workout,
            elapsed_secs: 0,
            running: false,
        }
    }

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn pause(&mut self) {
        self.running = false;
    }

    pub fn reset(&mut self) {
        self.elapsed_secs = 0;
        self.running = false;
    }

    pub fn tick(&mut self) -> WorkoutPhase {
        if self.running {
            self.elapsed_secs += 1;
        }

        self.phase()
    }

    pub fn phase(&self) -> WorkoutPhase {
        if self.elapsed_secs == 0 {
            return WorkoutPhase::NotStarted;
        }

        let mut remaining = self.elapsed_secs - 1;
        for (index, step) in self.workout.steps.iter().enumerate() {
            if remaining < step.duration_secs {
                return WorkoutPhase::Active {
                    step_index: index,
                    step_elapsed_secs: remaining + 1,
                    step: step.clone(),
                };
            }

            remaining -= step.duration_secs;
        }

        WorkoutPhase::Finished
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.elapsed_secs
    }

    pub fn workout(&self) -> &Workout {
        &self.workout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_through_steps() {
        let workout = Workout::new(
            "Test",
            vec![
                WorkoutStep::new("One", 2, 100),
                WorkoutStep::new("Two", 1, 200),
            ],
        );
        let mut engine = WorkoutEngine::new(workout);
        engine.start();

        assert!(matches!(
            engine.tick(),
            WorkoutPhase::Active { step_index: 0, .. }
        ));
        assert!(matches!(
            engine.tick(),
            WorkoutPhase::Active { step_index: 0, .. }
        ));
        assert!(matches!(
            engine.tick(),
            WorkoutPhase::Active { step_index: 1, .. }
        ));
        assert_eq!(engine.tick(), WorkoutPhase::Finished);
    }
}
