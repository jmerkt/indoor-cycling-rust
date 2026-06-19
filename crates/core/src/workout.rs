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

    /// Creates an interval session with an easy warmup and cooldown. The work and
    /// recovery blocks vary slightly on every call while the requested duration
    /// and training intent remain the guardrails.
    pub fn randomized(parameters: WorkoutParameters) -> Self {
        let total_secs = u64::from(parameters.duration_minutes.clamp(15, 180)) * 60;
        let warmup_secs = (total_secs / 6).clamp(300, 600);
        let cooldown_secs = (total_secs / 8).clamp(300, 600);
        let work_budget = total_secs.saturating_sub(warmup_secs + cooldown_secs);
        let mut random = WorkoutRandom::new();
        let mut steps = vec![WorkoutStep::new(
            "Warmup",
            warmup_secs,
            random.jitter(55, 8),
        )];
        let (work_secs, recovery_secs, base_effort) = parameters.effort_profile();
        let (efforts_per_set, set_recovery_secs) = parameters.set_profile();
        let recovery_scale_percent = parameters.recovery_scale_percent();
        let recovery_secs = scale_duration(recovery_secs, recovery_scale_percent);
        let set_recovery_secs = scale_duration(set_recovery_secs, recovery_scale_percent);
        let mut remaining = work_budget;
        let mut interval = 1;
        let mut efforts_in_set = 0;
        let mut set = 1;

        while remaining >= 90 {
            let effort_duration = random
                .jitter(work_secs, parameters.variation as u16 * 10)
                .clamp(30, remaining.saturating_sub(30) as u16);
            let effort_percent = random
                .jitter(base_effort, parameters.variation as u16 * 3)
                .clamp(55, 150);
            steps.push(WorkoutStep::new(
                format!("Effort {interval}"),
                u64::from(effort_duration),
                effort_percent,
            ));
            remaining = remaining.saturating_sub(u64::from(effort_duration));

            if remaining < 45 {
                break;
            }

            efforts_in_set += 1;
            let is_set_break = efforts_in_set >= efforts_per_set
                && remaining >= u64::from(set_recovery_secs.saturating_add(30));
            let base_recovery_secs = if is_set_break {
                set_recovery_secs
            } else {
                recovery_secs
            };
            let recovery_duration = random
                .jitter(base_recovery_secs, parameters.variation as u16 * 12)
                .clamp(30, remaining as u16);
            let recovery_percent = random
                .jitter(50, parameters.variation as u16 * 3)
                .clamp(40, 65);
            steps.push(WorkoutStep::new(
                if is_set_break {
                    format!("Set Recovery {set}")
                } else {
                    format!("Recovery {interval}")
                },
                u64::from(recovery_duration),
                recovery_percent,
            ));
            remaining = remaining.saturating_sub(u64::from(recovery_duration));
            if is_set_break {
                efforts_in_set = 0;
                set += 1;
            }
            interval += 1;
        }

        if remaining > 0 {
            steps.push(WorkoutStep::new("Endurance", remaining, 70));
        }
        steps.push(WorkoutStep::new(
            "Cooldown",
            cooldown_secs,
            random.jitter(45, 6),
        ));
        Self::new(
            format!(
                "{} {} Intervals",
                parameters.difficulty.label(),
                parameters.effort_duration.label()
            ),
            steps,
        )
    }

    pub fn total_duration_secs(&self) -> u64 {
        self.steps.iter().map(|step| step.duration_secs).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkoutDifficulty {
    Gentle,
    Easy,
    Moderate,
    Hard,
    VeryHard,
}

impl WorkoutDifficulty {
    fn label(self) -> &'static str {
        match self {
            Self::Gentle => "Gentle",
            Self::Easy => "Easy",
            Self::Moderate => "Moderate",
            Self::Hard => "Hard",
            Self::VeryHard => "Very Hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortDuration {
    VeryShort,
    Short,
    Medium,
    Long,
    VeryLong,
}

impl EffortDuration {
    fn label(self) -> &'static str {
        match self {
            Self::VeryShort => "Very Short",
            Self::Short => "Short",
            Self::Medium => "Medium",
            Self::Long => "Long",
            Self::VeryLong => "Very Long",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkoutParameters {
    pub duration_minutes: u16,
    pub difficulty: WorkoutDifficulty,
    pub effort_duration: EffortDuration,
    /// 1 is subtle variety; higher values allow more change in each block.
    pub variation: u8,
}

impl WorkoutParameters {
    fn effort_profile(self) -> (u16, u16, u16) {
        let (work_secs, recovery_secs) = match self.effort_duration {
            EffortDuration::VeryShort => (30, 60),
            EffortDuration::Short => (60, 75),
            EffortDuration::Medium => (300, 180),
            EffortDuration::Long => (600, 240),
            EffortDuration::VeryLong => (900, 300),
        };
        let effort_percent = match (self.difficulty, self.effort_duration) {
            (WorkoutDifficulty::Gentle, EffortDuration::VeryShort) => 100,
            (WorkoutDifficulty::Gentle, EffortDuration::Short) => 78,
            (WorkoutDifficulty::Gentle, EffortDuration::Medium) => 75,
            (WorkoutDifficulty::Gentle, EffortDuration::Long) => 72,
            (WorkoutDifficulty::Gentle, EffortDuration::VeryLong) => 70,
            (WorkoutDifficulty::Easy, EffortDuration::VeryShort) => 115,
            (WorkoutDifficulty::Easy, EffortDuration::Short) => 92,
            (WorkoutDifficulty::Easy, EffortDuration::Medium) => 86,
            (WorkoutDifficulty::Easy, EffortDuration::Long) => 80,
            (WorkoutDifficulty::Easy, EffortDuration::VeryLong) => 76,
            (WorkoutDifficulty::Moderate, EffortDuration::VeryShort) => 125,
            (WorkoutDifficulty::Moderate, EffortDuration::Short) => 105,
            (WorkoutDifficulty::Moderate, EffortDuration::Medium) => 94,
            (WorkoutDifficulty::Moderate, EffortDuration::Long) => 88,
            (WorkoutDifficulty::Moderate, EffortDuration::VeryLong) => 84,
            (WorkoutDifficulty::Hard, EffortDuration::VeryShort) => 135,
            (WorkoutDifficulty::Hard, EffortDuration::Short) => 120,
            (WorkoutDifficulty::Hard, EffortDuration::Medium) => 105,
            (WorkoutDifficulty::Hard, EffortDuration::Long) => 96,
            (WorkoutDifficulty::Hard, EffortDuration::VeryLong) => 91,
            (WorkoutDifficulty::VeryHard, EffortDuration::VeryShort) => 145,
            (WorkoutDifficulty::VeryHard, EffortDuration::Short) => 135,
            (WorkoutDifficulty::VeryHard, EffortDuration::Medium) => 115,
            (WorkoutDifficulty::VeryHard, EffortDuration::Long) => 105,
            (WorkoutDifficulty::VeryHard, EffortDuration::VeryLong) => 98,
        };
        (work_secs, recovery_secs, effort_percent)
    }

    fn set_profile(self) -> (u16, u16) {
        match self.effort_duration {
            EffortDuration::VeryShort => (5, 180),
            EffortDuration::Short => (4, 240),
            EffortDuration::Medium => (2, 300),
            EffortDuration::Long | EffortDuration::VeryLong => (2, 360),
        }
    }

    fn recovery_scale_percent(self) -> u16 {
        match self.difficulty {
            WorkoutDifficulty::Gentle => 140,
            WorkoutDifficulty::Easy => 120,
            WorkoutDifficulty::Moderate => 100,
            WorkoutDifficulty::Hard => 85,
            WorkoutDifficulty::VeryHard => 70,
        }
    }
}

fn scale_duration(duration_secs: u16, percent: u16) -> u16 {
    let scaled = u32::from(duration_secs) * u32::from(percent) / 100;
    u16::try_from(scaled).unwrap_or(u16::MAX).max(30)
}

struct WorkoutRandom(u64);

impl WorkoutRandom {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9);
        Self(nanos ^ 0xa076_1d64_78bd_642f)
    }

    fn jitter(&mut self, center: u16, radius: u16) -> u16 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        let offset = (self.0 >> 32) as u16 % (radius.saturating_mul(2) + 1);
        center.saturating_add(offset).saturating_sub(radius)
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

    #[test]
    fn randomized_workout_has_warmup_cooldown_and_requested_duration() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            difficulty: WorkoutDifficulty::Hard,
            effort_duration: EffortDuration::Short,
            variation: 3,
        });

        assert_eq!(workout.total_duration_secs(), 45 * 60);
        assert_eq!(workout.steps.first().unwrap().name, "Warmup");
        assert_eq!(workout.steps.last().unwrap().name, "Cooldown");
        assert!(workout
            .steps
            .iter()
            .any(|step| step.name.starts_with("Recovery")));
        assert!(workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Effort"))
            .all(|step| step.target_ftp_percent > 100));
    }

    #[test]
    fn randomized_workout_places_long_recovery_between_sets() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            difficulty: WorkoutDifficulty::Moderate,
            effort_duration: EffortDuration::Medium,
            variation: 2,
        });

        let set_recovery = workout
            .steps
            .iter()
            .find(|step| step.name.starts_with("Set Recovery"))
            .expect("a sufficiently long workout should contain a set recovery");
        assert!(set_recovery.duration_secs >= 270);
        assert!(set_recovery.target_ftp_percent <= 65);
    }

    #[test]
    fn difficulty_scales_recovery_duration() {
        let gentle = WorkoutParameters {
            duration_minutes: 45,
            difficulty: WorkoutDifficulty::Gentle,
            effort_duration: EffortDuration::Medium,
            variation: 1,
        };
        let very_hard = WorkoutParameters {
            difficulty: WorkoutDifficulty::VeryHard,
            ..gentle
        };

        assert!(gentle.recovery_scale_percent() > very_hard.recovery_scale_percent());
        assert!(
            scale_duration(300, gentle.recovery_scale_percent())
                > scale_duration(300, very_hard.recovery_scale_percent())
        );
    }
}
