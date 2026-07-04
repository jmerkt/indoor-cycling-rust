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
        let parameters = parameters.normalized();
        let total_secs = u64::from(parameters.duration_minutes.clamp(15, 180)) * 60;
        let warmup_secs = parameters.workout_type.warmup_secs(total_secs);
        let cooldown_secs = parameters.workout_type.cooldown_secs(total_secs);
        let work_budget = total_secs.saturating_sub(warmup_secs + cooldown_secs);
        let mut random = WorkoutRandom::new();
        let mut steps = vec![WorkoutStep::new(
            "Warmup",
            warmup_secs,
            random.jitter(parameters.workout_type.warmup_target(), 5),
        )];

        match parameters.workout_type {
            WorkoutType::Recovery => build_steady_session(
                &mut steps,
                work_budget,
                &mut random,
                parameters,
                SteadySpec {
                    block_name: "Recovery Spin",
                    relief_name: None,
                    block_secs: [180, 240, 300, 360, 420, 480, 600, 720, 900, 1050, 1200],
                    target_low: 40,
                    target_high: 55,
                    relief_target_low: 35,
                    relief_target_high: 45,
                },
            ),
            WorkoutType::Endurance => build_steady_session(
                &mut steps,
                work_budget,
                &mut random,
                parameters,
                SteadySpec {
                    block_name: "Endurance",
                    relief_name: Some("Easy Spin"),
                    block_secs: [300, 420, 600, 720, 900, 1050, 1200, 1500, 1800, 2100, 2400],
                    target_low: 58,
                    target_high: 75,
                    relief_target_low: 45,
                    relief_target_high: 60,
                },
            ),
            WorkoutType::Tempo => build_interval_session(
                &mut steps,
                work_budget,
                &mut random,
                parameters,
                IntervalSpec::tempo(),
            ),
            WorkoutType::SweetSpot => build_interval_session(
                &mut steps,
                work_budget,
                &mut random,
                parameters,
                IntervalSpec::sweet_spot(),
            ),
            WorkoutType::Threshold => build_interval_session(
                &mut steps,
                work_budget,
                &mut random,
                parameters,
                IntervalSpec::threshold(),
            ),
            WorkoutType::OverUnder => {
                build_over_under_session(&mut steps, work_budget, &mut random, parameters)
            }
            WorkoutType::Vo2Max => build_interval_session(
                &mut steps,
                work_budget,
                &mut random,
                parameters,
                IntervalSpec::vo2_max(),
            ),
            WorkoutType::Sprinting => build_interval_session(
                &mut steps,
                work_budget,
                &mut random,
                parameters,
                IntervalSpec::sprinting(),
            ),
            WorkoutType::Microbursts => {
                build_microburst_session(&mut steps, work_budget, &mut random, parameters)
            }
        }

        steps.push(WorkoutStep::new(
            "Cooldown",
            cooldown_secs,
            random.jitter(parameters.workout_type.cooldown_target(), 5),
        ));
        Self::new(parameters.workout_type.workout_name(), steps)
    }

    pub fn total_duration_secs(&self) -> u64 {
        self.steps.iter().map(|step| step.duration_secs).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortIntensity {
    Gentle,
    Easy,
    Moderate,
    Hard,
    VeryHard,
}

pub type WorkoutDifficulty = EffortIntensity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortDuration {
    VeryShort,
    Short,
    Medium,
    Long,
    VeryLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryIntensity {
    VeryEasy,
    Easy,
    Moderate,
    Steady,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDuration {
    VeryShort,
    Short,
    Medium,
    Long,
    VeryLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkoutType {
    Recovery,
    Endurance,
    Tempo,
    SweetSpot,
    Threshold,
    OverUnder,
    Vo2Max,
    Sprinting,
    Microbursts,
}

impl WorkoutType {
    fn workout_name(self) -> &'static str {
        match self {
            Self::Recovery => "Recovery Ride",
            Self::Endurance => "Endurance Ride",
            Self::Tempo => "Tempo Intervals",
            Self::SweetSpot => "Sweet Spot Intervals",
            Self::Threshold => "Threshold Intervals",
            Self::OverUnder => "Over-Under Intervals",
            Self::Vo2Max => "VO2 Max Intervals",
            Self::Sprinting => "Sprint Intervals",
            Self::Microbursts => "Microburst Intervals",
        }
    }

    fn warmup_secs(self, total_secs: u64) -> u64 {
        match self {
            Self::Recovery | Self::Endurance => (total_secs / 10).clamp(180, 480),
            Self::Tempo | Self::SweetSpot => (total_secs / 7).clamp(300, 600),
            Self::Threshold
            | Self::OverUnder
            | Self::Vo2Max
            | Self::Sprinting
            | Self::Microbursts => (total_secs / 6).clamp(420, 720),
        }
    }

    fn cooldown_secs(self, total_secs: u64) -> u64 {
        match self {
            Self::Recovery | Self::Endurance => (total_secs / 12).clamp(180, 420),
            _ => (total_secs / 8).clamp(300, 600),
        }
    }

    fn warmup_target(self) -> u16 {
        match self {
            Self::Recovery => 42,
            Self::Endurance => 50,
            Self::Tempo | Self::SweetSpot => 55,
            Self::Threshold
            | Self::OverUnder
            | Self::Vo2Max
            | Self::Sprinting
            | Self::Microbursts => 58,
        }
    }

    fn cooldown_target(self) -> u16 {
        match self {
            Self::Recovery => 38,
            _ => 45,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkoutParameters {
    pub duration_minutes: u16,
    pub workout_type: WorkoutType,
    /// Effort target level from 1 to 11.
    pub effort_intensity: u8,
    /// Recovery target level from 1 to 11.
    pub recovery_intensity: u8,
    /// Effort block duration level from 1 to 11.
    pub effort_duration: u8,
    /// Recovery block duration level from 1 to 11.
    pub recovery_duration: u8,
    /// 1 is subtle variety; 11 allows large changes in each block.
    pub variation: u8,
}

impl WorkoutParameters {
    fn normalized(self) -> Self {
        Self {
            duration_minutes: self.duration_minutes.clamp(15, 180),
            workout_type: self.workout_type,
            effort_intensity: self.effort_intensity.clamp(1, 11),
            recovery_intensity: self.recovery_intensity.clamp(1, 11),
            effort_duration: self.effort_duration.clamp(1, 11),
            recovery_duration: self.recovery_duration.clamp(1, 11),
            variation: self.variation.clamp(1, 11),
        }
    }
}

fn duration_level_secs(level: u8, values: &[u16; 11]) -> u16 {
    values[usize::from(level_index(level))]
}

fn level_index(level: u8) -> u8 {
    level.clamp(1, 11) - 1
}

fn level_to_percent(level: u8, low: u16, high: u16) -> u16 {
    let span = high.saturating_sub(low);
    low + (span * u16::from(level_index(level)) + 5) / 10
}

fn duration_jitter_radius(base_secs: u16, variation: u8) -> u16 {
    let radius = u32::from(base_secs) * u32::from(level_index(variation)) / 35;
    u16::try_from(radius).unwrap_or(u16::MAX)
}

fn power_jitter_radius(variation: u8, multiplier: u16) -> u16 {
    1 + u16::from(level_index(variation)) * multiplier / 4
}

fn push_fill(steps: &mut Vec<WorkoutStep>, name: &str, duration_secs: u64, target_percent: u16) {
    if duration_secs > 0 {
        steps.push(WorkoutStep::new(name, duration_secs, target_percent));
    }
}

#[derive(Clone, Copy)]
struct SteadySpec {
    block_name: &'static str,
    relief_name: Option<&'static str>,
    block_secs: [u16; 11],
    target_low: u16,
    target_high: u16,
    relief_target_low: u16,
    relief_target_high: u16,
}

fn build_steady_session(
    steps: &mut Vec<WorkoutStep>,
    mut remaining: u64,
    random: &mut WorkoutRandom,
    parameters: WorkoutParameters,
    spec: SteadySpec,
) {
    let base_block_secs = duration_level_secs(parameters.effort_duration, &spec.block_secs);
    let target = level_to_percent(
        parameters.effort_intensity,
        spec.target_low,
        spec.target_high,
    );
    let relief_target = level_to_percent(
        parameters.recovery_intensity,
        spec.relief_target_low,
        spec.relief_target_high,
    );
    let relief_secs = duration_level_secs(
        parameters.recovery_duration,
        &[60, 90, 120, 150, 180, 240, 300, 360, 420, 480, 600],
    );
    let mut block = 1;

    while remaining > 0 {
        let duration =
            jitter_duration(random, base_block_secs, parameters.variation, 60, remaining);
        let percent = random
            .jitter(target, power_jitter_radius(parameters.variation, 1))
            .clamp(spec.target_low, spec.target_high);
        steps.push(WorkoutStep::new(
            format!("{} {block}", spec.block_name),
            duration,
            percent,
        ));
        remaining = remaining.saturating_sub(duration);

        if let Some(relief_name) = spec.relief_name {
            if remaining >= 180 {
                let relief_duration =
                    jitter_duration(random, relief_secs, parameters.variation, 60, remaining);
                let relief_percent = random
                    .jitter(relief_target, power_jitter_radius(parameters.variation, 1))
                    .clamp(spec.relief_target_low, spec.relief_target_high);
                steps.push(WorkoutStep::new(
                    format!("{relief_name} {block}"),
                    relief_duration,
                    relief_percent,
                ));
                remaining = remaining.saturating_sub(relief_duration);
            }
        }

        block += 1;
    }
}

#[derive(Clone, Copy)]
struct IntervalSpec {
    effort_name: &'static str,
    recovery_name: &'static str,
    filler_name: &'static str,
    effort_secs: [u16; 11],
    recovery_secs: [u16; 11],
    effort_low: u16,
    effort_high: u16,
    recovery_low: u16,
    recovery_high: u16,
    min_effort_secs: u16,
    max_effort_secs: u16,
    efforts_per_set: u16,
    set_recovery_multiplier: u16,
    power_jitter_multiplier: u16,
}

impl IntervalSpec {
    fn tempo() -> Self {
        Self {
            effort_name: "Tempo",
            recovery_name: "Recovery",
            filler_name: "Endurance",
            effort_secs: [180, 240, 300, 360, 420, 480, 600, 720, 900, 1050, 1200],
            recovery_secs: [60, 90, 120, 150, 180, 240, 300, 360, 420, 480, 600],
            effort_low: 76,
            effort_high: 90,
            recovery_low: 45,
            recovery_high: 65,
            min_effort_secs: 120,
            max_effort_secs: 1500,
            efforts_per_set: 3,
            set_recovery_multiplier: 2,
            power_jitter_multiplier: 1,
        }
    }

    fn sweet_spot() -> Self {
        Self {
            effort_name: "Sweet Spot",
            recovery_name: "Recovery",
            filler_name: "Endurance",
            effort_secs: [240, 300, 360, 420, 480, 600, 720, 900, 1020, 1200, 1500],
            recovery_secs: [60, 90, 120, 150, 180, 240, 300, 360, 420, 480, 600],
            effort_low: 88,
            effort_high: 94,
            recovery_low: 40,
            recovery_high: 60,
            min_effort_secs: 180,
            max_effort_secs: 1800,
            efforts_per_set: 2,
            set_recovery_multiplier: 2,
            power_jitter_multiplier: 1,
        }
    }

    fn threshold() -> Self {
        Self {
            effort_name: "Threshold",
            recovery_name: "Recovery",
            filler_name: "Endurance",
            effort_secs: [180, 240, 300, 360, 420, 480, 600, 720, 900, 1020, 1200],
            recovery_secs: [120, 150, 180, 210, 240, 300, 360, 420, 480, 600, 720],
            effort_low: 95,
            effort_high: 105,
            recovery_low: 40,
            recovery_high: 60,
            min_effort_secs: 180,
            max_effort_secs: 1200,
            efforts_per_set: 2,
            set_recovery_multiplier: 2,
            power_jitter_multiplier: 1,
        }
    }

    fn vo2_max() -> Self {
        Self {
            effort_name: "VO2 Max",
            recovery_name: "Recovery",
            filler_name: "Endurance",
            effort_secs: [45, 60, 75, 90, 120, 150, 180, 210, 240, 270, 300],
            recovery_secs: [60, 75, 90, 120, 150, 180, 210, 240, 270, 300, 360],
            effort_low: 106,
            effort_high: 120,
            recovery_low: 35,
            recovery_high: 55,
            min_effort_secs: 45,
            max_effort_secs: 360,
            efforts_per_set: 4,
            set_recovery_multiplier: 2,
            power_jitter_multiplier: 1,
        }
    }

    fn sprinting() -> Self {
        Self {
            effort_name: "Sprint",
            recovery_name: "Easy Spin",
            filler_name: "Easy Spin",
            effort_secs: [10, 12, 15, 20, 25, 30, 35, 40, 45, 60, 75],
            recovery_secs: [90, 120, 150, 180, 210, 240, 300, 360, 420, 480, 600],
            effort_low: 125,
            effort_high: 170,
            recovery_low: 30,
            recovery_high: 50,
            min_effort_secs: 10,
            max_effort_secs: 90,
            efforts_per_set: 5,
            set_recovery_multiplier: 2,
            power_jitter_multiplier: 3,
        }
    }
}

fn build_interval_session(
    steps: &mut Vec<WorkoutStep>,
    mut remaining: u64,
    random: &mut WorkoutRandom,
    parameters: WorkoutParameters,
    spec: IntervalSpec,
) {
    let base_effort_secs = duration_level_secs(parameters.effort_duration, &spec.effort_secs);
    let base_recovery_secs = duration_level_secs(parameters.recovery_duration, &spec.recovery_secs);
    let effort_target = level_to_percent(
        parameters.effort_intensity,
        spec.effort_low,
        spec.effort_high,
    );
    let recovery_target = level_to_percent(
        parameters.recovery_intensity,
        spec.recovery_low,
        spec.recovery_high,
    );
    let mut interval = 1;
    let mut set = 1;
    let mut intervals_in_set = 0;

    while remaining >= u64::from(spec.min_effort_secs) {
        let reserve_recovery = if remaining >= u64::from(spec.min_effort_secs + 60) {
            30
        } else {
            0
        };
        let max_effort = remaining
            .saturating_sub(reserve_recovery)
            .min(u64::from(spec.max_effort_secs));
        let effort_duration = jitter_duration(
            random,
            base_effort_secs,
            parameters.variation,
            spec.min_effort_secs,
            max_effort,
        );
        let effort_percent = random
            .jitter(
                effort_target,
                power_jitter_radius(parameters.variation, spec.power_jitter_multiplier),
            )
            .clamp(spec.effort_low, spec.effort_high);
        steps.push(WorkoutStep::new(
            format!("{} {interval}", spec.effort_name),
            effort_duration,
            effort_percent,
        ));
        remaining = remaining.saturating_sub(effort_duration);

        if remaining < 45 {
            break;
        }

        intervals_in_set += 1;
        let is_set_recovery = intervals_in_set >= spec.efforts_per_set && remaining >= 120;
        let recovery_base = if is_set_recovery {
            base_recovery_secs.saturating_mul(spec.set_recovery_multiplier)
        } else {
            base_recovery_secs
        };
        let recovery_duration =
            jitter_duration(random, recovery_base, parameters.variation, 30, remaining);
        let recovery_percent = random
            .jitter(
                recovery_target,
                power_jitter_radius(parameters.variation, 1),
            )
            .clamp(spec.recovery_low, spec.recovery_high);
        steps.push(WorkoutStep::new(
            if is_set_recovery {
                format!("Set Recovery {set}")
            } else {
                format!("{} {interval}", spec.recovery_name)
            },
            recovery_duration,
            recovery_percent,
        ));
        remaining = remaining.saturating_sub(recovery_duration);

        if is_set_recovery {
            intervals_in_set = 0;
            set += 1;
        }
        interval += 1;
    }

    push_fill(steps, spec.filler_name, remaining, 65);
}

fn build_over_under_session(
    steps: &mut Vec<WorkoutStep>,
    mut remaining: u64,
    random: &mut WorkoutRandom,
    parameters: WorkoutParameters,
) {
    let under_secs = duration_level_secs(
        parameters.effort_duration,
        &[45, 60, 75, 90, 120, 150, 180, 210, 240, 300, 360],
    );
    let over_secs = (under_secs / 2).clamp(30, 180);
    let recovery_secs = duration_level_secs(
        parameters.recovery_duration,
        &[90, 120, 150, 180, 210, 240, 300, 360, 420, 480, 600],
    );
    let under_target = level_to_percent(parameters.effort_intensity, 88, 96);
    let over_target = level_to_percent(parameters.effort_intensity, 102, 112);
    let recovery_target = level_to_percent(parameters.recovery_intensity, 40, 60);
    let repeats_per_set = match parameters.effort_duration {
        1..=3 => 6,
        4..=7 => 5,
        _ => 4,
    };
    let mut set = 1;

    while remaining >= u64::from(under_secs + over_secs + 60) {
        for repeat in 1..=repeats_per_set {
            if remaining < u64::from(under_secs + over_secs) {
                break;
            }
            let under_duration = jitter_duration(
                random,
                under_secs,
                parameters.variation,
                30,
                remaining.saturating_sub(u64::from(over_secs)),
            );
            let under_percent = random
                .jitter(under_target, power_jitter_radius(parameters.variation, 1))
                .clamp(88, 96);
            steps.push(WorkoutStep::new(
                format!("Under {set}.{repeat}"),
                under_duration,
                under_percent,
            ));
            remaining = remaining.saturating_sub(under_duration);

            if remaining < 30 {
                break;
            }

            let over_duration =
                jitter_duration(random, over_secs, parameters.variation, 30, remaining);
            let over_percent = random
                .jitter(over_target, power_jitter_radius(parameters.variation, 1))
                .clamp(102, 112);
            steps.push(WorkoutStep::new(
                format!("Over {set}.{repeat}"),
                over_duration,
                over_percent,
            ));
            remaining = remaining.saturating_sub(over_duration);
        }

        if remaining < 60 {
            break;
        }

        let recovery_duration =
            jitter_duration(random, recovery_secs, parameters.variation, 60, remaining);
        let recovery_percent = random
            .jitter(
                recovery_target,
                power_jitter_radius(parameters.variation, 1),
            )
            .clamp(40, 60);
        steps.push(WorkoutStep::new(
            format!("Set Recovery {set}"),
            recovery_duration,
            recovery_percent,
        ));
        remaining = remaining.saturating_sub(recovery_duration);
        set += 1;
    }

    push_fill(steps, "Endurance", remaining, 65);
}

fn build_microburst_session(
    steps: &mut Vec<WorkoutStep>,
    mut remaining: u64,
    random: &mut WorkoutRandom,
    parameters: WorkoutParameters,
) {
    let effort_secs = duration_level_secs(
        parameters.effort_duration,
        &[15, 20, 25, 30, 30, 35, 40, 40, 45, 50, 60],
    );
    let float_secs = duration_level_secs(
        parameters.recovery_duration,
        &[10, 15, 20, 20, 30, 30, 40, 45, 45, 60, 75],
    );
    let set_recovery_secs = duration_level_secs(
        parameters.recovery_duration,
        &[120, 150, 180, 210, 240, 300, 360, 420, 480, 540, 600],
    );
    let effort_target = level_to_percent(parameters.effort_intensity, 110, 140);
    let float_target = level_to_percent(parameters.recovery_intensity, 35, 85);
    let set_recovery_target = level_to_percent(parameters.recovery_intensity, 35, 60);
    let min_cycle_secs = u64::from(effort_secs + float_secs);
    let base_repeats_per_set = (480 / (effort_secs + float_secs)).clamp(4, 14);
    let repeat_radius = u16::from(level_index(parameters.variation)) / 5;
    let mut set = 1;

    while remaining >= min_cycle_secs {
        let repeats_this_set = random
            .jitter(base_repeats_per_set, repeat_radius)
            .clamp(4, 16);

        for repeat in 1..=repeats_this_set {
            if remaining < min_cycle_secs {
                break;
            }

            let effort_duration = jitter_short_duration(
                random,
                effort_secs,
                parameters.variation,
                10,
                remaining.saturating_sub(u64::from(float_secs)),
            );
            let effort_percent = random
                .jitter(effort_target, power_jitter_radius(parameters.variation, 2))
                .clamp(110, 140);
            steps.push(WorkoutStep::new(
                format!("Microburst {set}.{repeat}"),
                effort_duration,
                effort_percent,
            ));
            remaining = remaining.saturating_sub(effort_duration);

            if remaining < 10 {
                break;
            }

            let float_duration =
                jitter_short_duration(random, float_secs, parameters.variation, 10, remaining);
            let float_percent = random
                .jitter(float_target, power_jitter_radius(parameters.variation, 1))
                .clamp(35, 85);
            steps.push(WorkoutStep::new(
                format!("Float {set}.{repeat}"),
                float_duration,
                float_percent,
            ));
            remaining = remaining.saturating_sub(float_duration);
        }

        if remaining < min_cycle_secs + 60 {
            break;
        }

        let recovery_duration = jitter_duration(
            random,
            set_recovery_secs,
            parameters.variation,
            60,
            remaining.saturating_sub(min_cycle_secs),
        );
        let recovery_percent = random
            .jitter(
                set_recovery_target,
                power_jitter_radius(parameters.variation, 1),
            )
            .clamp(35, 60);
        steps.push(WorkoutStep::new(
            format!("Set Recovery {set}"),
            recovery_duration,
            recovery_percent,
        ));
        remaining = remaining.saturating_sub(recovery_duration);
        set += 1;
    }

    push_fill(steps, "Easy Spin", remaining, set_recovery_target);
}

fn jitter_duration(
    random: &mut WorkoutRandom,
    base_secs: u16,
    variation: u8,
    minimum_secs: u16,
    remaining_secs: u64,
) -> u64 {
    let maximum_secs = remaining_secs.min(u64::from(u16::MAX)) as u16;
    if maximum_secs <= minimum_secs {
        return u64::from(maximum_secs);
    }
    u64::from(
        random
            .jitter(base_secs, duration_jitter_radius(base_secs, variation))
            .clamp(minimum_secs, maximum_secs),
    )
}

fn jitter_short_duration(
    random: &mut WorkoutRandom,
    base_secs: u16,
    variation: u8,
    minimum_secs: u16,
    remaining_secs: u64,
) -> u64 {
    let maximum_secs = remaining_secs.min(u64::from(u16::MAX)) as u16;
    if maximum_secs <= minimum_secs {
        return u64::from(maximum_secs);
    }
    let radius = if level_index(variation) == 0 {
        0
    } else {
        duration_jitter_radius(base_secs, variation).max(2) / 2
    };
    u64::from(
        random
            .jitter(base_secs, radius)
            .clamp(minimum_secs, maximum_secs),
    )
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
            workout_type: WorkoutType::SweetSpot,
            effort_intensity: 11,
            recovery_intensity: 6,
            effort_duration: 7,
            recovery_duration: 6,
            variation: 3,
        });

        assert_eq!(workout.total_duration_secs(), 45 * 60);
        assert_eq!(workout.steps.first().unwrap().name, "Warmup");
        assert_eq!(workout.steps.last().unwrap().name, "Cooldown");
        assert!(workout
            .steps
            .iter()
            .any(|step| step.name.starts_with("Sweet Spot")));
        assert!(workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Sweet Spot"))
            .all(|step| (88..=94).contains(&step.target_ftp_percent)));
    }

    #[test]
    fn randomized_workout_places_long_recovery_between_sets() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            workout_type: WorkoutType::Threshold,
            effort_intensity: 6,
            recovery_intensity: 6,
            effort_duration: 7,
            recovery_duration: 8,
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
    fn vo2_workout_keeps_efforts_in_vo2_range() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            workout_type: WorkoutType::Vo2Max,
            effort_intensity: 11,
            recovery_intensity: 6,
            effort_duration: 11,
            recovery_duration: 6,
            variation: 5,
        });

        let efforts: Vec<_> = workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("VO2 Max"))
            .collect();
        assert!(!efforts.is_empty());
        assert!(efforts
            .iter()
            .all(|step| (106..=120).contains(&step.target_ftp_percent)));
        assert!(efforts.iter().all(|step| step.duration_secs <= 360));
    }

    #[test]
    fn sprint_workout_keeps_efforts_short_and_anaerobic() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            workout_type: WorkoutType::Sprinting,
            effort_intensity: 11,
            recovery_intensity: 4,
            effort_duration: 11,
            recovery_duration: 8,
            variation: 4,
        });

        let sprints: Vec<_> = workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Sprint"))
            .collect();
        assert!(!sprints.is_empty());
        assert!(sprints.iter().all(|step| step.target_ftp_percent >= 125));
        assert!(sprints.iter().all(|step| step.duration_secs <= 90));
    }

    #[test]
    fn recovery_workout_stays_easy() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            workout_type: WorkoutType::Recovery,
            effort_intensity: 11,
            recovery_intensity: 11,
            effort_duration: 6,
            recovery_duration: 6,
            variation: 11,
        });

        assert!(workout
            .steps
            .iter()
            .filter(|step| !matches!(step.name.as_str(), "Warmup" | "Cooldown"))
            .all(|step| step.target_ftp_percent <= 55));
    }

    #[test]
    fn all_common_workout_types_keep_requested_duration() {
        for workout_type in [
            WorkoutType::Recovery,
            WorkoutType::Endurance,
            WorkoutType::Tempo,
            WorkoutType::SweetSpot,
            WorkoutType::Threshold,
            WorkoutType::OverUnder,
            WorkoutType::Vo2Max,
            WorkoutType::Sprinting,
            WorkoutType::Microbursts,
        ] {
            let workout = Workout::randomized(WorkoutParameters {
                duration_minutes: 50,
                workout_type,
                effort_intensity: 6,
                recovery_intensity: 6,
                effort_duration: 6,
                recovery_duration: 6,
                variation: 6,
            });

            assert_eq!(workout.total_duration_secs(), 50 * 60);
            assert_eq!(workout.steps.first().unwrap().name, "Warmup");
            assert_eq!(workout.steps.last().unwrap().name, "Cooldown");
        }
    }

    #[test]
    fn over_under_workout_contains_both_sides_of_threshold() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 50,
            workout_type: WorkoutType::OverUnder,
            effort_intensity: 8,
            recovery_intensity: 6,
            effort_duration: 6,
            recovery_duration: 6,
            variation: 4,
        });

        assert!(workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Under"))
            .all(|step| (88..=96).contains(&step.target_ftp_percent)));
        assert!(workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Over"))
            .all(|step| (102..=112).contains(&step.target_ftp_percent)));
        assert!(workout
            .steps
            .iter()
            .any(|step| step.name.starts_with("Under")));
        assert!(workout
            .steps
            .iter()
            .any(|step| step.name.starts_with("Over")));
    }

    #[test]
    fn microburst_workout_can_create_forty_twenty_blocks() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            workout_type: WorkoutType::Microbursts,
            effort_intensity: 6,
            recovery_intensity: 11,
            effort_duration: 7,
            recovery_duration: 3,
            variation: 1,
        });

        let bursts: Vec<_> = workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Microburst"))
            .collect();
        let floats: Vec<_> = workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Float"))
            .collect();

        assert!(!bursts.is_empty());
        assert_eq!(bursts.len(), floats.len());
        assert!(bursts.iter().all(|step| step.duration_secs == 40));
        assert!(floats.iter().all(|step| step.duration_secs == 20));
        assert!(bursts
            .iter()
            .all(|step| (110..=140).contains(&step.target_ftp_percent)));
        assert!(floats
            .iter()
            .all(|step| (35..=85).contains(&step.target_ftp_percent)));
    }

    #[test]
    fn microburst_workout_can_create_thirty_thirty_blocks() {
        let workout = Workout::randomized(WorkoutParameters {
            duration_minutes: 45,
            workout_type: WorkoutType::Microbursts,
            effort_intensity: 8,
            recovery_intensity: 3,
            effort_duration: 4,
            recovery_duration: 5,
            variation: 1,
        });

        let bursts: Vec<_> = workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Microburst"))
            .collect();
        let floats: Vec<_> = workout
            .steps
            .iter()
            .filter(|step| step.name.starts_with("Float"))
            .collect();

        assert!(!bursts.is_empty());
        assert_eq!(bursts.len(), floats.len());
        assert!(bursts.iter().all(|step| step.duration_secs == 30));
        assert!(floats.iter().all(|step| step.duration_secs == 30));
    }
}
