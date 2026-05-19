use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use indoor_cycling_core::{RideRecorder, Workout, WorkoutEngine, WorkoutPhase};
use indoor_cycling_devices::{SimulatedTrainer, TrainerDevice};

slint::include_modules!();

struct AppState {
    recorder: RideRecorder,
    workout: WorkoutEngine,
    trainer: SimulatedTrainer,
    running: bool,
}

impl AppState {
    fn new() -> Self {
        let mut trainer = SimulatedTrainer::new();
        trainer.connect().expect("simulated trainer should connect");

        Self {
            recorder: RideRecorder::new(),
            workout: WorkoutEngine::new(Workout::endurance_sample()),
            trainer,
            running: false,
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    let state = Rc::new(RefCell::new(AppState::new()));
    let timer = slint::Timer::default();

    window.set_workout_name(state.borrow().workout.workout().name.clone().into());
    window.set_ride_status("Ready".into());

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        window.on_start_ride(move || start_ride(&window_weak, &state));
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        window.on_pause_ride(move || {
            let mut state = state.borrow_mut();
            state.running = false;
            state.workout.pause();
            if let Some(window) = window_weak.upgrade() {
                window.set_ride_status("Paused".into());
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        window.on_reset_ride(move || {
            let mut state = state.borrow_mut();
            *state = AppState::new();
            if let Some(window) = window_weak.upgrade() {
                render(&window, &state);
                window.set_ride_status("Ready".into());
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                if let Some(window) = window_weak.upgrade() {
                    tick(&window, &state);
                }
            },
        );
    }

    window.run()
}

fn start_ride(window: &slint::Weak<AppWindow>, state: &Rc<RefCell<AppState>>) {
    let mut state = state.borrow_mut();
    state.running = true;
    state.workout.start();

    if let Some(window) = window.upgrade() {
        window.set_ride_status("Riding".into());
    }
}

fn tick(window: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let mut state = state.borrow_mut();
    if !state.running {
        return;
    }

    let phase = state.workout.tick();
    match phase {
        WorkoutPhase::Active { step, .. } => {
            let _ = state.trainer.set_target_power(step.target_power_watts);
            window.set_current_step(step.name.into());
            window.set_target_power_watts(step.target_power_watts.into());
        }
        WorkoutPhase::Finished => {
            state.running = false;
            state.workout.pause();
            window.set_ride_status("Finished".into());
            return;
        }
        WorkoutPhase::NotStarted => {}
    }

    if let Ok(sample) = state.trainer.next_sample() {
        state.recorder.push(sample);
    }

    render(window, &state);
}

fn render(window: &AppWindow, state: &AppState) {
    let stats = state.recorder.stats();
    let latest = state.recorder.latest();

    window.set_elapsed_text(format_duration(stats.elapsed_secs).into());
    window.set_power_watts(
        latest
            .map(|sample| sample.power_watts)
            .unwrap_or_default()
            .into(),
    );
    window.set_cadence_rpm(
        latest
            .map(|sample| sample.cadence_rpm)
            .unwrap_or_default()
            .into(),
    );
    window.set_heart_rate_bpm(
        latest
            .and_then(|sample| sample.heart_rate_bpm)
            .map(i32::from)
            .unwrap_or_default(),
    );
    window.set_average_power_text(format!("{:.0} W", stats.average_power_watts).into());
}

fn format_duration(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}
