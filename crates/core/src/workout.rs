#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkoutStep {
    pub name: String,
    pub duration_secs: u64,
    pub target_ftp_percent: u16,
}

impl WorkoutStep {
    pub fn new(name: impl Into<String>, duration_secs: u64, target_ftp_percent: u16) -> Self {
        Self {
            name: name.into(),
            duration_secs,
            target_ftp_percent,
        }
    }

    pub fn target_power_watts(&self, ftp_watts: u16) -> u16 {
        let scaled = u32::from(ftp_watts) * u32::from(self.target_ftp_percent);
        u16::try_from((scaled + 50) / 100).unwrap_or(u16::MAX)
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
                WorkoutStep::new("Warmup", 300, 50),
                WorkoutStep::new("Steady", 900, 70),
                WorkoutStep::new("Tempo", 300, 85),
                WorkoutStep::new("Cooldown", 300, 45),
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

    pub fn skip_to_next_step(&mut self) {
        self.elapsed_secs = match self.phase() {
            WorkoutPhase::NotStarted => self
                .step_start_elapsed_secs(1)
                .unwrap_or_else(|| self.finished_elapsed_secs()),
            WorkoutPhase::Active { step_index, .. } => self
                .step_start_elapsed_secs(step_index + 1)
                .unwrap_or_else(|| self.finished_elapsed_secs()),
            WorkoutPhase::Finished => self.finished_elapsed_secs(),
        };
    }

    pub fn go_to_previous_step(&mut self) {
        self.elapsed_secs = match self.phase() {
            WorkoutPhase::NotStarted => 0,
            WorkoutPhase::Active { step_index, .. } => {
                let previous_index = step_index.saturating_sub(1);
                self.step_start_elapsed_secs(previous_index)
                    .unwrap_or_default()
            }
            WorkoutPhase::Finished => self
                .workout
                .steps
                .len()
                .checked_sub(1)
                .and_then(|index| self.step_start_elapsed_secs(index))
                .unwrap_or_default(),
        };
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

    fn step_start_elapsed_secs(&self, step_index: usize) -> Option<u64> {
        if step_index >= self.workout.steps.len() {
            return None;
        }

        Some(
            self.workout.steps[..step_index]
                .iter()
                .map(|step| step.duration_secs)
                .sum::<u64>()
                + 1,
        )
    }

    fn finished_elapsed_secs(&self) -> u64 {
        self.workout.total_duration_secs().saturating_add(1)
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

    #[test]
    fn scales_step_target_with_ftp() {
        let step = WorkoutStep::new("Tempo", 60, 85);

        assert_eq!(step.target_power_watts(250), 213);
    }

    #[test]
    fn navigates_between_steps() {
        let workout = Workout::new(
            "Test",
            vec![
                WorkoutStep::new("One", 2, 50),
                WorkoutStep::new("Two", 3, 75),
                WorkoutStep::new("Three", 1, 100),
            ],
        );
        let mut engine = WorkoutEngine::new(workout);

        engine.skip_to_next_step();
        assert!(matches!(
            engine.phase(),
            WorkoutPhase::Active { step_index: 1, .. }
        ));

        engine.skip_to_next_step();
        assert!(matches!(
            engine.phase(),
            WorkoutPhase::Active { step_index: 2, .. }
        ));

        engine.go_to_previous_step();
        assert!(matches!(
            engine.phase(),
            WorkoutPhase::Active { step_index: 1, .. }
        ));

        engine.go_to_previous_step();
        assert!(matches!(
            engine.phase(),
            WorkoutPhase::Active { step_index: 0, .. }
        ));
    }

    #[test]
    fn can_return_from_finished_workout() {
        let workout = Workout::new("Test", vec![WorkoutStep::new("Only", 1, 100)]);
        let mut engine = WorkoutEngine::new(workout);
        engine.start();
        engine.tick();
        assert_eq!(engine.tick(), WorkoutPhase::Finished);

        engine.go_to_previous_step();

        assert!(matches!(
            engine.phase(),
            WorkoutPhase::Active { step_index: 0, .. }
        ));
    }
}
