use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use indoor_cycling_core::{RideRecorder, Workout, WorkoutEngine, WorkoutPhase};
use indoor_cycling_devices::{
    BluetoothDeviceService, DeviceId, DeviceKind, DiscoveryEvent, SimulatedTrainer, TrainerDevice,
};
use slint::{Model, ModelRc, SharedString, VecModel};

slint::include_modules!();

struct AppState {
    recorder: RideRecorder,
    workout: WorkoutEngine,
    trainer: SimulatedTrainer,
    external_heart_rate_bpm: Option<u16>,
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
            external_heart_rate_bpm: None,
            running: false,
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    let state = Rc::new(RefCell::new(AppState::new()));
    let ride_timer = slint::Timer::default();
    let discovery_timer = slint::Timer::default();
    let discovered_devices = Rc::new(VecModel::<DeviceListItem>::default());
    let bluetooth_service = BluetoothDeviceService::start().map(Rc::new);

    window.set_workout_name(state.borrow().workout.workout().name.clone().into());
    window.set_ride_status("Ready".into());
    window.set_discovered_devices(ModelRc::from(Rc::clone(&discovered_devices)));

    if let Err(error) = &bluetooth_service {
        window.set_device_scan_available(false);
        window.set_device_scan_error(true);
        window.set_device_scan_status(format!("Error: {error}").into());
    }

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
        ride_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                if let Some(window) = window_weak.upgrade() {
                    tick(&window, &state);
                }
            },
        );
    }

    if let Ok(bluetooth_service) = &bluetooth_service {
        let service = Rc::clone(bluetooth_service);
        let window_weak = window.as_weak();
        let scan_devices_model = Rc::clone(&discovered_devices);
        window.on_scan_devices(move || {
            scan_devices_model.set_vec(Vec::new());
            if let Some(window) = window_weak.upgrade() {
                window.set_device_scan_error(false);
                window.set_device_scan_running(true);
                window.set_device_scan_status("Starting Bluetooth scan...".into());
            }

            if let Err(error) = service.scan(Duration::from_secs(5)) {
                if let Some(window) = window_weak.upgrade() {
                    window.set_device_scan_error(true);
                    window.set_device_scan_running(false);
                    window.set_device_scan_status(format!("Error: {error}").into());
                }
            }
        });

        let service = Rc::clone(bluetooth_service);
        let window_weak = window.as_weak();
        window.on_connect_heart_rate(move |device_id| {
            let device_id = DeviceId::new(device_id.as_str());
            if let Some(window) = window_weak.upgrade() {
                window.set_connecting_heart_rate_id(device_id.as_str().into());
                window.set_heart_rate_connection_error(false);
                window.set_heart_rate_connection_status("Connecting...".into());
            }

            if let Err(error) = service.connect_heart_rate(device_id) {
                if let Some(window) = window_weak.upgrade() {
                    window.set_connecting_heart_rate_id("".into());
                    window.set_heart_rate_connection_error(true);
                    window.set_heart_rate_connection_status(format!("Error: {error}").into());
                }
            }
        });

        let service = Rc::clone(bluetooth_service);
        let window_weak = window.as_weak();
        window.on_disconnect_heart_rate(move || {
            if let Err(error) = service.disconnect_heart_rate() {
                if let Some(window) = window_weak.upgrade() {
                    window.set_heart_rate_connection_error(true);
                    window.set_heart_rate_connection_status(format!("Error: {error}").into());
                }
            }
        });

        let service = Rc::clone(bluetooth_service);
        let window_weak = window.as_weak();
        let event_devices_model = Rc::clone(&discovered_devices);
        let state = Rc::clone(&state);
        discovery_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                let Some(window) = window_weak.upgrade() else {
                    return;
                };

                while let Ok(event) = service.try_recv() {
                    handle_discovery_event(&window, &event_devices_model, &state, event);
                }
            },
        );
    }

    window.run()
}

fn handle_discovery_event(
    window: &AppWindow,
    devices: &Rc<VecModel<DeviceListItem>>,
    state: &Rc<RefCell<AppState>>,
    event: DiscoveryEvent,
) {
    match event {
        DiscoveryEvent::ScanStarted => {
            window.set_device_scan_error(false);
            window.set_device_scan_running(true);
            window.set_device_scan_status("Scanning for compatible devices...".into());
        }
        DiscoveryEvent::DeviceDiscovered(device) => {
            let supports_heart_rate = device.kind.is_heart_rate_monitor();
            devices.push(DeviceListItem {
                id: SharedString::from(device.id.as_str()),
                name: SharedString::from(device.name),
                kind: SharedString::from(device_kind_label(device.kind)),
                signal: SharedString::from(signal_label(device.signal_strength)),
                supports_heart_rate,
            });
        }
        DiscoveryEvent::ScanFinished => {
            window.set_device_scan_running(false);
            if window.get_device_scan_error() {
                return;
            }

            let count = devices.row_count();
            let status = match count {
                0 => "Scan complete. No compatible devices found.".to_owned(),
                1 => "Scan complete. Found 1 compatible device.".to_owned(),
                _ => format!("Scan complete. Found {count} compatible devices."),
            };
            window.set_device_scan_status(status.into());
        }
        DiscoveryEvent::Error(error) => {
            window.set_device_scan_error(true);
            window.set_device_scan_running(false);
            window.set_device_scan_status(format!("Error: {error}").into());
        }
        DiscoveryEvent::HeartRateConnecting(device_id) => {
            window.set_connecting_heart_rate_id(device_id.as_str().into());
            window.set_heart_rate_connection_error(false);
            window.set_heart_rate_connection_status("Connecting...".into());
        }
        DiscoveryEvent::HeartRateConnected(device_id) => {
            window.set_connecting_heart_rate_id("".into());
            window.set_connected_heart_rate_id(device_id.as_str().into());
            window.set_heart_rate_connection_error(false);
            window.set_heart_rate_connection_status("Connected, waiting for data...".into());
        }
        DiscoveryEvent::HeartRateMeasurement(value) => {
            state.borrow_mut().external_heart_rate_bpm = Some(value);
            window.set_live_heart_rate_bpm(i32::from(value));
            window.set_heart_rate_bpm(i32::from(value));
            window.set_heart_rate_connection_status("Connected and receiving data".into());
        }
        DiscoveryEvent::HeartRateDisconnected(_) => {
            state.borrow_mut().external_heart_rate_bpm = None;
            window.set_connecting_heart_rate_id("".into());
            window.set_connected_heart_rate_id("".into());
            window.set_live_heart_rate_bpm(0);
            window.set_heart_rate_connection_error(false);
            window.set_heart_rate_connection_status("No heart rate monitor connected".into());
            render(window, &state.borrow());
        }
        DiscoveryEvent::HeartRateError(error) => {
            state.borrow_mut().external_heart_rate_bpm = None;
            window.set_connecting_heart_rate_id("".into());
            window.set_connected_heart_rate_id("".into());
            window.set_live_heart_rate_bpm(0);
            window.set_heart_rate_connection_error(true);
            window.set_heart_rate_connection_status(format!("Error: {error}").into());
        }
    }
}

fn device_kind_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Trainer => "FTMS trainer",
        DeviceKind::HeartRateMonitor => "Heart rate monitor",
        DeviceKind::TrainerAndHeartRateMonitor => "FTMS trainer and heart rate monitor",
    }
}

fn signal_label(signal_strength: Option<i16>) -> String {
    signal_strength
        .map(|rssi| format!("{rssi} dBm"))
        .unwrap_or_else(|| "Signal unknown".to_owned())
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

    if let Ok(mut sample) = state.trainer.next_sample() {
        if let Some(heart_rate_bpm) = state.external_heart_rate_bpm {
            sample.heart_rate_bpm = u8::try_from(heart_rate_bpm).ok();
        }
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
        state
            .external_heart_rate_bpm
            .map(i32::from)
            .or_else(|| {
                latest
                    .and_then(|sample| sample.heart_rate_bpm)
                    .map(i32::from)
            })
            .unwrap_or_default(),
    );
    window.set_average_power_text(format!("{:.0} W", stats.average_power_watts).into());
}

fn format_duration(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}
