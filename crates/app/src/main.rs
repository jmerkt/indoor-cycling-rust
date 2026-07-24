use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use indoor_cycling_core::{
    RideRecorder, Workout, WorkoutEngine, WorkoutParameters, WorkoutPhase, WorkoutType,
};
use indoor_cycling_devices::{
    BluetoothDeviceService, DeviceCapabilities, DeviceId, DeviceMeasurement, DeviceRole,
    DiscoveryEvent, SimulatedTrainer, TrainerDevice,
};
use indoor_cycling_storage::{
    AppConfiguration, ConfigurationStore, RideFileStore, StoredWorkout, StoredWorkoutStep,
    WindowConfiguration, WorkoutBuilderConfiguration,
};
use slint::{Model, ModelRc, SharedString, VecModel};

slint::include_modules!();

const DEFAULT_FTP_WATTS: u16 = 200;
const CONFIGURATION_VERSION: u8 = 3;

struct AppState {
    recorder: RideRecorder,
    workout: WorkoutEngine,
    trainer: SimulatedTrainer,
    ftp_watts: u16,
    workout_builder: WorkoutBuilderConfiguration,
    ride_file_store: Option<RideFileStore>,
    window: Option<WindowConfiguration>,
    smart_trainer_connected: bool,
    erg_enabled: bool,
    erg_command_pending: bool,
    erg_target_watts: Option<u16>,
    erg_error: Option<String>,
    external_power_watts: Option<i16>,
    secondary_power_watts: Option<i16>,
    external_heart_rate_bpm: Option<u16>,
    external_cadence_rpm: Option<u16>,
    running: bool,
    finish_dialog_resume_running: bool,
}

impl AppState {
    fn new(configuration: Option<&AppConfiguration>) -> Self {
        let mut trainer = SimulatedTrainer::new();
        trainer.connect().expect("simulated trainer should connect");

        let ftp_watts = configuration
            .map(|configuration| configuration.ftp_watts.clamp(50, 600))
            .unwrap_or(DEFAULT_FTP_WATTS);
        let workout_builder = configuration
            .map(|configuration| {
                let builder = &configuration.workout_builder;
                if configuration.version < 2 {
                    legacy_workout_builder_configuration(
                        builder.length_minutes,
                        builder.workout_type,
                        builder.effort_intensity,
                        builder.recovery_intensity,
                        builder.effort_duration,
                        builder.recovery_duration,
                        builder.variation,
                    )
                } else {
                    workout_builder_configuration(
                        builder.length_minutes,
                        builder.workout_type,
                        builder.effort_intensity,
                        builder.recovery_intensity,
                        builder.effort_duration,
                        builder.recovery_duration,
                        builder.variation,
                    )
                }
            })
            .unwrap_or_else(default_workout_builder_configuration);
        let workout = configuration
            .map(|configuration| workout_from_stored(&configuration.latest_workout))
            .filter(|workout| !workout.steps.is_empty())
            .unwrap_or_else(Workout::endurance_sample);

        Self {
            recorder: RideRecorder::new(),
            workout: WorkoutEngine::new(workout),
            trainer,
            ftp_watts,
            workout_builder,
            ride_file_store: RideFileStore::for_current_user().ok(),
            window: configuration.and_then(|configuration| configuration.window.clone()),
            smart_trainer_connected: false,
            erg_enabled: false,
            erg_command_pending: false,
            erg_target_watts: None,
            erg_error: None,
            external_power_watts: None,
            secondary_power_watts: None,
            external_heart_rate_bpm: None,
            external_cadence_rpm: None,
            running: false,
            finish_dialog_resume_running: false,
        }
    }

    fn configuration(&self) -> AppConfiguration {
        AppConfiguration {
            version: CONFIGURATION_VERSION,
            ftp_watts: self.ftp_watts,
            workout_builder: self.workout_builder.clone(),
            latest_workout: stored_workout(self.workout.workout()),
            window: self.window.clone(),
        }
    }

    fn reset_ride(&mut self) {
        let mut trainer = SimulatedTrainer::new();
        trainer.connect().expect("simulated trainer should connect");
        self.recorder = RideRecorder::new();
        self.workout.reset();
        self.trainer = trainer;
        self.running = false;
        self.finish_dialog_resume_running = false;
    }

    fn replace_workout(&mut self, workout: Workout) {
        let was_running = self.running;
        self.workout = WorkoutEngine::new(workout);
        if was_running {
            self.workout.start();
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    let configuration_store = ConfigurationStore::for_current_user().ok();
    let saved_configuration = configuration_store
        .as_ref()
        .and_then(|store| match store.load() {
            Ok(configuration) => configuration,
            Err(error) => {
                eprintln!("Could not load application settings: {error}");
                None
            }
        });
    let saved_configuration =
        saved_configuration.filter(|configuration| configuration.version <= CONFIGURATION_VERSION);
    let state = Rc::new(RefCell::new(AppState::new(saved_configuration.as_ref())));
    let ride_timer = slint::Timer::default();
    let discovery_timer = slint::Timer::default();
    let discovered_devices = Rc::new(VecModel::<DeviceListItem>::default());
    let bluetooth_service = BluetoothDeviceService::start().map(Rc::new);
    let bluetooth_service_handle = bluetooth_service.as_ref().ok().cloned();

    window.set_discovered_devices(ModelRc::from(Rc::clone(&discovered_devices)));
    if let Some(window_size) = state.borrow().window.as_ref().filter(|size| {
        size.logical_width.is_finite()
            && size.logical_height.is_finite()
            && size.logical_width >= 640.
            && size.logical_height >= 480.
    }) {
        window.window().set_size(slint::LogicalSize::new(
            window_size.logical_width,
            window_size.logical_height,
        ));
    }
    let builder = state.borrow().workout_builder.clone();
    window.set_draft_length_minutes(builder.length_minutes);
    window.set_draft_workout_type(builder.workout_type);
    window.set_draft_effort_intensity(builder.effort_intensity);
    window.set_draft_recovery_intensity(builder.recovery_intensity);
    window.set_draft_effort_duration(builder.effort_duration);
    window.set_draft_recovery_duration(builder.recovery_duration);
    window.set_draft_variation(builder.variation);
    window.set_ride_status("Ready".into());
    render(&window, &state.borrow());

    if let Err(error) = &bluetooth_service {
        window.set_device_scan_available(false);
        window.set_device_scan_error(true);
        window.set_device_scan_status(format!("Error: {error}").into());
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let configuration_store = configuration_store.clone();
        let bluetooth_service = bluetooth_service_handle.clone();
        window.window().on_close_requested(move || {
            if let Some(window) = window_weak.upgrade() {
                let window_api = window.window();
                let size = window_api.size().to_logical(window_api.scale_factor());
                let mut state = state.borrow_mut();
                state.window = Some(WindowConfiguration {
                    logical_width: size.width,
                    logical_height: size.height,
                });
                persist_configuration(configuration_store.as_ref(), &state);
            }
            if let Some(bluetooth_service) = &bluetooth_service {
                bluetooth_service.shutdown();
            }
            slint::CloseRequestResponse::HideWindow
        });
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
        let state = Rc::clone(&state);
        let configuration_store = configuration_store.clone();
        window.on_builder_settings_changed(
            move |length_minutes,
                  workout_type,
                  effort_intensity,
                  recovery_intensity,
                  effort_duration,
                  recovery_duration,
                  variation| {
                let mut state = state.borrow_mut();
                state.workout_builder = workout_builder_configuration(
                    length_minutes,
                    workout_type,
                    effort_intensity,
                    recovery_intensity,
                    effort_duration,
                    recovery_duration,
                    variation,
                );
                persist_configuration(configuration_store.as_ref(), &state);
            },
        );
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        let configuration_store = configuration_store.clone();
        window.on_set_ftp(move |ftp_watts| {
            let mut state = state.borrow_mut();
            state.ftp_watts = u16::try_from(ftp_watts.clamp(50, 600)).unwrap_or(DEFAULT_FTP_WATTS);
            apply_current_target(&mut state);
            sync_erg_target(&mut state, bluetooth_service.as_deref());
            persist_configuration(configuration_store.as_ref(), &state);
            if let Some(window) = window_weak.upgrade() {
                render(&window, &state);
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        let configuration_store = configuration_store.clone();
        window.on_randomize_workout(
            move |length_minutes,
                  workout_type,
                  effort_intensity,
                  recovery_intensity,
                  effort_duration,
                  recovery_duration,
                  variation| {
                let builder = workout_builder_configuration(
                    length_minutes,
                    workout_type,
                    effort_intensity,
                    recovery_intensity,
                    effort_duration,
                    recovery_duration,
                    variation,
                );
                let parameters = WorkoutParameters {
                    duration_minutes: u16::try_from(builder.length_minutes).unwrap_or(45),
                    workout_type: workout_type_from_level(builder.workout_type),
                    effort_intensity: workout_level(builder.effort_intensity),
                    recovery_intensity: workout_level(builder.recovery_intensity),
                    effort_duration: workout_level(builder.effort_duration),
                    recovery_duration: workout_level(builder.recovery_duration),
                    variation: workout_level(builder.variation),
                };
                let mut state = state.borrow_mut();
                let was_running = state.running;
                let has_recording = !state.recorder.samples().is_empty();
                state.workout_builder = builder;
                state.replace_workout(Workout::randomized(parameters));
                apply_current_target(&mut state);
                sync_erg_target(&mut state, bluetooth_service.as_deref());
                persist_configuration(configuration_store.as_ref(), &state);
                if let Some(window) = window_weak.upgrade() {
                    window.set_ride_status(
                        if was_running {
                            "Riding"
                        } else if has_recording {
                            "Paused"
                        } else {
                            "Ready"
                        }
                        .into(),
                    );
                    render(&window, &state);
                }
            },
        );
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        window.on_previous_stage(move || {
            let mut state = state.borrow_mut();
            state.workout.go_to_previous_step();
            apply_current_target(&mut state);
            sync_erg_target(&mut state, bluetooth_service.as_deref());
            if !state.running {
                if let Some(window) = window_weak.upgrade() {
                    window.set_ride_status("Paused".into());
                    render(&window, &state);
                }
            } else if let Some(window) = window_weak.upgrade() {
                render(&window, &state);
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        window.on_skip_stage(move || {
            let mut state = state.borrow_mut();
            state.workout.skip_to_next_step();
            if matches!(state.workout.phase(), WorkoutPhase::Finished) {
                state.running = false;
                state.workout.pause();
                disable_erg(&mut state, bluetooth_service.as_deref());
            }
            apply_current_target(&mut state);
            sync_erg_target(&mut state, bluetooth_service.as_deref());
            if let Some(window) = window_weak.upgrade() {
                if matches!(state.workout.phase(), WorkoutPhase::Finished) {
                    window.set_ride_status("Finished".into());
                }
                render(&window, &state);
            }
        });
    }

    if let Some(bluetooth_service) = bluetooth_service_handle.clone() {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        window.on_toggle_erg_mode(move || {
            let mut state = state.borrow_mut();
            if !state.smart_trainer_connected || state.erg_command_pending {
                return;
            }

            let enabled = !state.erg_enabled;
            let target_power_watts = if enabled {
                let Some(target_power_watts) = current_target_power(&state) else {
                    state.erg_error = Some("no workout target is available".to_owned());
                    if let Some(window) = window_weak.upgrade() {
                        render(&window, &state);
                    }
                    return;
                };
                target_power_watts
            } else {
                0
            };
            match bluetooth_service.set_erg_mode(enabled, target_power_watts) {
                Ok(()) => {
                    state.erg_command_pending = true;
                    state.erg_error = None;
                    if let Some(window) = window_weak.upgrade() {
                        window.set_erg_status(if enabled {
                            "Enabling ERG...".into()
                        } else {
                            "Disabling ERG...".into()
                        });
                        render(&window, &state);
                    }
                }
                Err(error) => {
                    state.erg_error = Some(error.to_string());
                    if let Some(window) = window_weak.upgrade() {
                        render(&window, &state);
                    }
                }
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        window.on_finish_ride(move || {
            let mut state = state.borrow_mut();
            state.finish_dialog_resume_running = state.running;
            state.running = false;
            state.workout.pause();
            disable_erg(&mut state, bluetooth_service.as_deref());
            if let Some(window) = window_weak.upgrade() {
                window.set_finish_dialog_open(true);
                window.set_finish_error("".into());
                window.set_ride_status("Paused".into());
                render(&window, &state);
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        window.on_resume_finish_dialog(move || {
            let mut state = state.borrow_mut();
            let resume_running = state.finish_dialog_resume_running;
            state.finish_dialog_resume_running = false;
            if resume_running {
                state.running = true;
                state.workout.start();
            }
            if let Some(window) = window_weak.upgrade() {
                window.set_finish_dialog_open(false);
                window.set_finish_error("".into());
                window.set_ride_status(if resume_running { "Riding" } else { "Paused" }.into());
                render(&window, &state);
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        window.on_finish_and_discard(move || {
            let mut state = state.borrow_mut();
            state.reset_ride();
            sync_erg_target(&mut state, bluetooth_service.as_deref());
            if let Some(window) = window_weak.upgrade() {
                window.set_finish_dialog_open(false);
                window.set_finish_error("".into());
                render(&window, &state);
                window.set_ride_status("Ready".into());
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        window.on_finish_and_save(move || {
            let mut state = state.borrow_mut();
            match save_finished_ride(&state) {
                Ok(path) => {
                    state.reset_ride();
                    sync_erg_target(&mut state, bluetooth_service.as_deref());
                    if let Some(window) = window_weak.upgrade() {
                        window.set_finish_dialog_open(false);
                        window.set_finish_error("".into());
                        render(&window, &state);
                        window.set_ride_status(format!("Saved {}", path.display()).into());
                    }
                }
                Err(error) => {
                    if let Some(window) = window_weak.upgrade() {
                        window.set_finish_error(format!("Save failed: {error}").into());
                    }
                }
            }
        });
    }

    {
        let window_weak = window.as_weak();
        let state = Rc::clone(&state);
        let bluetooth_service = bluetooth_service_handle.clone();
        ride_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            move || {
                if let Some(window) = window_weak.upgrade() {
                    tick(&window, &state, bluetooth_service.as_deref());
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
        window.on_connect_device(move |device_id, role_index| {
            let Some(role) = role_from_index(role_index) else {
                return;
            };
            let device_id = DeviceId::new(device_id.as_str());
            if let Some(window) = window_weak.upgrade() {
                set_role_connecting(&window, role, device_id.as_str());
            }

            if let Err(error) = service.connect(role, device_id) {
                if let Some(window) = window_weak.upgrade() {
                    set_role_error(&window, role, &error.to_string());
                }
            }
        });

        let service = Rc::clone(bluetooth_service);
        let window_weak = window.as_weak();
        window.on_disconnect_device(move |role_index| {
            let Some(role) = role_from_index(role_index) else {
                return;
            };
            if let Err(error) = service.disconnect(role) {
                if let Some(window) = window_weak.upgrade() {
                    set_role_error(&window, role, &error.to_string());
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
                    handle_discovery_event(
                        &window,
                        &event_devices_model,
                        &state,
                        service.as_ref(),
                        event,
                    );
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
    bluetooth_service: &BluetoothDeviceService,
    event: DiscoveryEvent,
) {
    match event {
        DiscoveryEvent::ScanStarted => {
            window.set_device_scan_error(false);
            window.set_device_scan_running(true);
            window.set_device_scan_status("Scanning for compatible devices...".into());
        }
        DiscoveryEvent::DeviceDiscovered(device) => {
            let capabilities = device.capabilities;
            devices.push(DeviceListItem {
                id: SharedString::from(device.id.as_str()),
                name: SharedString::from(device.name),
                capabilities: SharedString::from(capabilities_label(capabilities)),
                signal: SharedString::from(signal_label(device.signal_strength)),
                supports_power: capabilities.power,
                supports_heart_rate: capabilities.heart_rate,
                supports_cadence: capabilities.cadence,
                supports_smart_trainer: capabilities.smart_trainer,
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
        DiscoveryEvent::RoleConnecting { role, device_id } => {
            set_role_connecting(window, role, device_id.as_str());
        }
        DiscoveryEvent::RoleConnected { role, device_id } => {
            if role == DeviceRole::SmartTrainer {
                let mut state = state.borrow_mut();
                state.smart_trainer_connected = true;
                state.erg_enabled = false;
                state.erg_command_pending = false;
                state.erg_target_watts = None;
                state.erg_error = None;
            }
            set_role_connected(window, role, device_id.as_str());
            render(window, &state.borrow());
        }
        DiscoveryEvent::Measurement { role, measurement } => {
            handle_measurement(window, state, role, measurement);
        }
        DiscoveryEvent::RoleDisconnected { role, .. } => {
            clear_role_measurement(state, role);
            if role == DeviceRole::SmartTrainer {
                clear_erg_state(&mut state.borrow_mut());
            }
            set_role_disconnected(window, role);
            render(window, &state.borrow());
        }
        DiscoveryEvent::RoleError { role, message } => {
            clear_role_measurement(state, role);
            if role == DeviceRole::SmartTrainer {
                clear_erg_state(&mut state.borrow_mut());
            }
            set_role_error(window, role, &message);
            render(window, &state.borrow());
        }
        DiscoveryEvent::ErgModeChanged {
            enabled,
            target_power_watts,
        } => {
            let mut state = state.borrow_mut();
            state.erg_enabled = enabled;
            state.erg_command_pending = false;
            state.erg_target_watts = target_power_watts;
            state.erg_error = None;
            sync_erg_target(&mut state, Some(bluetooth_service));
            render(window, &state);
        }
        DiscoveryEvent::TrainerControlError(message) => {
            let mut state = state.borrow_mut();
            state.erg_command_pending = false;
            state.erg_target_watts = None;
            state.erg_error = Some(message);
            render(window, &state);
        }
    }
}

fn capabilities_label(capabilities: DeviceCapabilities) -> String {
    let mut labels = Vec::new();
    if capabilities.power {
        labels.push("Power");
    }
    if capabilities.heart_rate {
        labels.push("Heart rate");
    }
    if capabilities.cadence {
        labels.push("RPM");
    }
    if capabilities.smart_trainer {
        labels.push("Smart trainer");
    }
    labels.join(" / ")
}

fn signal_label(signal_strength: Option<i16>) -> String {
    signal_strength
        .map(|rssi| format!("{rssi} dBm"))
        .unwrap_or_else(|| "Signal unknown".to_owned())
}

fn role_from_index(index: i32) -> Option<DeviceRole> {
    match index {
        0 => Some(DeviceRole::PowerSource),
        1 => Some(DeviceRole::SecondaryPowerSource),
        2 => Some(DeviceRole::HeartRate),
        3 => Some(DeviceRole::Cadence),
        4 => Some(DeviceRole::SmartTrainer),
        _ => None,
    }
}

fn set_role_connecting(window: &AppWindow, role: DeviceRole, device_id: &str) {
    match role {
        DeviceRole::PowerSource => {
            window.set_connecting_power_id(device_id.into());
            window.set_power_connection_error(false);
            window.set_power_connection_status("Connecting...".into());
        }
        DeviceRole::SecondaryPowerSource => {
            window.set_connecting_secondary_power_id(device_id.into());
            window.set_secondary_power_connection_error(false);
            window.set_secondary_power_connection_status("Connecting...".into());
        }
        DeviceRole::HeartRate => {
            window.set_connecting_heart_rate_id(device_id.into());
            window.set_heart_rate_connection_error(false);
            window.set_heart_rate_connection_status("Connecting...".into());
        }
        DeviceRole::Cadence => {
            window.set_connecting_cadence_id(device_id.into());
            window.set_cadence_connection_error(false);
            window.set_cadence_connection_status("Connecting...".into());
        }
        DeviceRole::SmartTrainer => {
            window.set_connecting_smart_trainer_id(device_id.into());
            window.set_smart_trainer_connection_error(false);
            window.set_smart_trainer_connection_status("Connecting...".into());
        }
    }
}

fn set_role_connected(window: &AppWindow, role: DeviceRole, device_id: &str) {
    match role {
        DeviceRole::PowerSource => {
            window.set_connecting_power_id("".into());
            window.set_connected_power_id(device_id.into());
            window.set_power_connection_error(false);
            window.set_power_connection_status("Connected, waiting for data...".into());
        }
        DeviceRole::SecondaryPowerSource => {
            window.set_connecting_secondary_power_id("".into());
            window.set_connected_secondary_power_id(device_id.into());
            window.set_secondary_power_connection_error(false);
            window.set_secondary_power_connection_status("Connected, waiting for data...".into());
        }
        DeviceRole::HeartRate => {
            window.set_connecting_heart_rate_id("".into());
            window.set_connected_heart_rate_id(device_id.into());
            window.set_heart_rate_connection_error(false);
            window.set_heart_rate_connection_status("Connected, waiting for data...".into());
        }
        DeviceRole::Cadence => {
            window.set_connecting_cadence_id("".into());
            window.set_connected_cadence_id(device_id.into());
            window.set_cadence_connection_error(false);
            window.set_cadence_connection_status("Connected, waiting for data...".into());
        }
        DeviceRole::SmartTrainer => {
            window.set_connecting_smart_trainer_id("".into());
            window.set_connected_smart_trainer_id(device_id.into());
            window.set_smart_trainer_connection_error(false);
            window.set_smart_trainer_connection_status("Connected".into());
        }
    }
}

fn set_role_disconnected(window: &AppWindow, role: DeviceRole) {
    match role {
        DeviceRole::PowerSource => {
            window.set_connecting_power_id("".into());
            window.set_connected_power_id("".into());
            window.set_live_power_watts(0);
            window.set_power_connection_error(false);
            window.set_power_connection_status("No primary power source connected".into());
        }
        DeviceRole::SecondaryPowerSource => {
            window.set_connecting_secondary_power_id("".into());
            window.set_connected_secondary_power_id("".into());
            window.set_live_secondary_power_watts(0);
            window.set_secondary_power_connection_error(false);
            window.set_secondary_power_connection_status(
                "No secondary power source connected".into(),
            );
        }
        DeviceRole::HeartRate => {
            window.set_connecting_heart_rate_id("".into());
            window.set_connected_heart_rate_id("".into());
            window.set_live_heart_rate_bpm(0);
            window.set_heart_rate_connection_error(false);
            window.set_heart_rate_connection_status("No heart rate monitor connected".into());
        }
        DeviceRole::Cadence => {
            window.set_connecting_cadence_id("".into());
            window.set_connected_cadence_id("".into());
            window.set_live_cadence_rpm(0);
            window.set_cadence_connection_error(false);
            window.set_cadence_connection_status("No cadence source connected".into());
        }
        DeviceRole::SmartTrainer => {
            window.set_connecting_smart_trainer_id("".into());
            window.set_connected_smart_trainer_id("".into());
            window.set_smart_trainer_connection_error(false);
            window.set_smart_trainer_connection_status("No smart trainer connected".into());
        }
    }
}

fn set_role_error(window: &AppWindow, role: DeviceRole, message: &str) {
    set_role_disconnected(window, role);
    let status = format!("Error: {message}");
    match role {
        DeviceRole::PowerSource => {
            window.set_power_connection_error(true);
            window.set_power_connection_status(status.into());
        }
        DeviceRole::SecondaryPowerSource => {
            window.set_secondary_power_connection_error(true);
            window.set_secondary_power_connection_status(status.into());
        }
        DeviceRole::HeartRate => {
            window.set_heart_rate_connection_error(true);
            window.set_heart_rate_connection_status(status.into());
        }
        DeviceRole::Cadence => {
            window.set_cadence_connection_error(true);
            window.set_cadence_connection_status(status.into());
        }
        DeviceRole::SmartTrainer => {
            window.set_smart_trainer_connection_error(true);
            window.set_smart_trainer_connection_status(status.into());
        }
    }
}

fn handle_measurement(
    window: &AppWindow,
    state: &Rc<RefCell<AppState>>,
    role: DeviceRole,
    measurement: DeviceMeasurement,
) {
    match (role, measurement) {
        (DeviceRole::PowerSource, DeviceMeasurement::PowerWatts(value)) => {
            state.borrow_mut().external_power_watts = Some(value);
            window.set_live_power_watts(i32::from(value.max(0)));
            window.set_power_watts(i32::from(value.max(0)));
            window.set_power_connection_status("Connected and receiving data".into());
        }
        (DeviceRole::SecondaryPowerSource, DeviceMeasurement::PowerWatts(value)) => {
            state.borrow_mut().secondary_power_watts = Some(value);
            window.set_live_secondary_power_watts(i32::from(value.max(0)));
            window.set_secondary_power_watts(i32::from(value.max(0)));
            window.set_secondary_power_connection_status("Connected and receiving data".into());
        }
        (DeviceRole::HeartRate, DeviceMeasurement::HeartRateBpm(value)) => {
            state.borrow_mut().external_heart_rate_bpm = Some(value);
            window.set_live_heart_rate_bpm(i32::from(value));
            window.set_heart_rate_bpm(i32::from(value));
            window.set_heart_rate_connection_status("Connected and receiving data".into());
        }
        (DeviceRole::Cadence, DeviceMeasurement::CadenceRpm(value)) => {
            state.borrow_mut().external_cadence_rpm = Some(value);
            window.set_live_cadence_rpm(i32::from(value));
            window.set_cadence_rpm(i32::from(value));
            window.set_cadence_connection_status("Connected and receiving data".into());
        }
        _ => {}
    }
}

fn clear_role_measurement(state: &Rc<RefCell<AppState>>, role: DeviceRole) {
    let mut state = state.borrow_mut();
    match role {
        DeviceRole::PowerSource => state.external_power_watts = None,
        DeviceRole::SecondaryPowerSource => state.secondary_power_watts = None,
        DeviceRole::HeartRate => state.external_heart_rate_bpm = None,
        DeviceRole::Cadence => state.external_cadence_rpm = None,
        DeviceRole::SmartTrainer => {}
    }
}

fn save_finished_ride(state: &AppState) -> std::io::Result<PathBuf> {
    let store = state.ride_file_store.as_ref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not determine where to save ride files",
        )
    })?;
    let samples = state.recorder.samples();
    if samples.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "there are no ride samples to save",
        ));
    }

    store.save_fit(&ride_id(state), samples)
}

fn ride_id(state: &AppState) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let mut workout_name = sanitize_filename(&state.workout.workout().name);
    if workout_name.is_empty() {
        workout_name = "workout".to_owned();
    }
    format!("{timestamp}-{workout_name}")
}

fn sanitize_filename(value: &str) -> String {
    let mut filename = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            filename.push(character.to_ascii_lowercase());
        } else if character.is_ascii_whitespace() || matches!(character, '-' | '_') {
            if !filename.ends_with('-') {
                filename.push('-');
            }
        }
    }
    filename.trim_matches('-').to_owned()
}

fn start_ride(window: &slint::Weak<AppWindow>, state: &Rc<RefCell<AppState>>) {
    let mut state = state.borrow_mut();
    state.running = true;
    state.workout.start();

    if let Some(window) = window.upgrade() {
        window.set_ride_status("Riding".into());
    }
}

fn tick(
    window: &AppWindow,
    state: &Rc<RefCell<AppState>>,
    bluetooth_service: Option<&BluetoothDeviceService>,
) {
    let mut state = state.borrow_mut();
    if !state.running {
        return;
    }

    let phase = state.workout.tick();
    match phase {
        WorkoutPhase::Active { step, .. } => {
            let ftp_watts = state.ftp_watts;
            let _ = state
                .trainer
                .set_target_power(step.target_power_watts(ftp_watts));
            sync_erg_target(&mut state, bluetooth_service);
        }
        WorkoutPhase::Finished => {
            state.running = false;
            state.workout.pause();
            disable_erg(&mut state, bluetooth_service);
            window.set_ride_status("Finished".into());
            render(window, &state);
            return;
        }
        WorkoutPhase::NotStarted => {}
    }

    if let Ok(mut sample) = state.trainer.next_sample() {
        if let Some(power_watts) = state.external_power_watts {
            sample.power_watts = u16::try_from(power_watts.max(0)).unwrap_or_default();
        }
        if let Some(cadence_rpm) = state.external_cadence_rpm {
            sample.cadence_rpm = cadence_rpm;
        }
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
    let phase = state.workout.phase();

    window.set_workout_name(state.workout.workout().name.clone().into());
    window.set_ftp_watts(i32::from(state.ftp_watts));
    window.set_erg_enabled(state.erg_enabled);
    window.set_erg_available(
        state.smart_trainer_connected
            && (state.erg_enabled || current_target_power(state).is_some()),
    );
    window.set_erg_pending(state.erg_command_pending);
    window.set_erg_error(state.erg_error.is_some());
    if let Some(error) = &state.erg_error {
        window.set_erg_status(format!("ERG error: {error}").into());
    } else if !state.smart_trainer_connected {
        window.set_erg_status("Connect a smart trainer".into());
    } else if state.erg_command_pending {
        window.set_erg_status("Updating ERG...".into());
    } else if state.erg_enabled {
        window.set_erg_status(
            format!(
                "ERG holding {} W",
                state.erg_target_watts.unwrap_or_default()
            )
            .into(),
        );
    } else {
        window.set_erg_status("ERG mode off".into());
    }
    window.set_elapsed_text(format_duration(stats.elapsed_secs).into());
    window.set_power_watts(
        state
            .external_power_watts
            .map(|value| i32::from(value.max(0)))
            .or_else(|| latest.map(|sample| i32::from(sample.power_watts)))
            .unwrap_or_default(),
    );
    window.set_secondary_power_watts(
        state
            .secondary_power_watts
            .map(|value| i32::from(value.max(0)))
            .unwrap_or_default(),
    );
    window.set_cadence_rpm(
        state
            .external_cadence_rpm
            .map(i32::from)
            .or_else(|| latest.map(|sample| i32::from(sample.cadence_rpm)))
            .unwrap_or_default(),
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
    render_workout(window, state, &phase);
}

fn apply_current_target(state: &mut AppState) {
    if let Some(target_power_watts) = current_target_power(state) {
        let _ = state.trainer.set_target_power(target_power_watts);
    }
}

fn current_target_power(state: &AppState) -> Option<u16> {
    match state.workout.phase() {
        WorkoutPhase::Active { step, .. } => Some(step.target_power_watts(state.ftp_watts)),
        WorkoutPhase::NotStarted => state
            .workout
            .workout()
            .steps
            .first()
            .map(|step| step.target_power_watts(state.ftp_watts)),
        WorkoutPhase::Finished => None,
    }
}

fn sync_erg_target(state: &mut AppState, bluetooth_service: Option<&BluetoothDeviceService>) {
    if !state.erg_enabled || state.erg_command_pending {
        return;
    }
    let Some(target_power_watts) = current_target_power(state) else {
        return;
    };
    if state.erg_target_watts == Some(target_power_watts) {
        return;
    }
    let Some(bluetooth_service) = bluetooth_service else {
        return;
    };

    if bluetooth_service
        .set_erg_target_power(target_power_watts)
        .is_ok()
    {
        state.erg_command_pending = true;
        state.erg_target_watts = Some(target_power_watts);
        state.erg_error = None;
    }
}

fn disable_erg(state: &mut AppState, bluetooth_service: Option<&BluetoothDeviceService>) {
    if !state.erg_enabled || state.erg_command_pending {
        return;
    }
    let Some(bluetooth_service) = bluetooth_service else {
        return;
    };

    match bluetooth_service.set_erg_mode(false, 0) {
        Ok(()) => {
            state.erg_command_pending = true;
            state.erg_error = None;
        }
        Err(error) => state.erg_error = Some(error.to_string()),
    }
}

fn clear_erg_state(state: &mut AppState) {
    state.smart_trainer_connected = false;
    state.erg_enabled = false;
    state.erg_command_pending = false;
    state.erg_target_watts = None;
    state.erg_error = None;
}

fn render_workout(window: &AppWindow, state: &AppState, phase: &WorkoutPhase) {
    let workout = state.workout.workout();
    let (active_index, active_elapsed_secs) = match phase {
        WorkoutPhase::Active {
            step_index,
            step_elapsed_secs,
            ..
        } => (Some(*step_index), *step_elapsed_secs),
        _ => (None, 0),
    };

    let stages = workout
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let completed =
                matches!(phase, WorkoutPhase::Finished) || active_index.is_some_and(|i| index < i);
            let active = active_index == Some(index);
            let progress_percent = if completed {
                100
            } else if active {
                ((active_elapsed_secs.saturating_mul(100) / step.duration_secs.max(1)).min(100))
                    as i32
            } else {
                0
            };

            WorkoutStageItem {
                name: step.name.clone().into(),
                duration_secs: i32::try_from(step.duration_secs).unwrap_or(i32::MAX),
                duration_text: format_duration(step.duration_secs).into(),
                target_percent: i32::from(step.target_ftp_percent),
                target_watts: i32::from(step.target_power_watts(state.ftp_watts)),
                progress_percent,
                active,
                completed,
            }
        })
        .collect::<Vec<_>>();
    window.set_workout_stages(ModelRc::from(Rc::new(VecModel::from(stages))));

    match phase {
        WorkoutPhase::NotStarted => {
            let first_step = workout.steps.first();
            set_timeline_stage_position(window, workout, first_step.map(|_| 0));
            window.set_target_power_watts(
                first_step
                    .map(|step| i32::from(step.target_power_watts(state.ftp_watts)))
                    .unwrap_or_default(),
            );
            window.set_current_stage_name(
                first_step
                    .map(|step| step.name.as_str())
                    .unwrap_or("No workout")
                    .into(),
            );
            window.set_current_stage_position(
                if first_step.is_some() {
                    format!("STAGE 1 OF {}", workout.steps.len())
                } else {
                    "NO STAGES".to_owned()
                }
                .into(),
            );
            window.set_step_remaining_text(
                first_step
                    .map(|step| format_duration(step.duration_secs))
                    .unwrap_or_else(|| "00:00".to_owned())
                    .into(),
            );
            window.set_current_target_percent(
                first_step
                    .map(|step| i32::from(step.target_ftp_percent))
                    .unwrap_or_default(),
            );
            window.set_workout_status(
                first_step
                    .map(|step| {
                        format!(
                            "Ready: {} at {} W ({}% FTP)",
                            step.name,
                            step.target_power_watts(state.ftp_watts),
                            step.target_ftp_percent
                        )
                    })
                    .unwrap_or_else(|| "No workout stages".to_owned())
                    .into(),
            );
            window.set_can_go_previous_stage(false);
            window.set_can_skip_stage(first_step.is_some());
        }
        WorkoutPhase::Active {
            step_index,
            step_elapsed_secs,
            step,
        } => {
            set_timeline_stage_position(window, workout, Some(*step_index));
            let remaining_secs = step.duration_secs.saturating_sub(*step_elapsed_secs);
            let target_watts = step.target_power_watts(state.ftp_watts);
            window.set_target_power_watts(i32::from(target_watts));
            window.set_current_stage_name(step.name.clone().into());
            window.set_current_stage_position(
                format!("STAGE {} OF {}", step_index + 1, workout.steps.len()).into(),
            );
            window.set_step_remaining_text(format_duration(remaining_secs).into());
            window.set_current_target_percent(i32::from(step.target_ftp_percent));
            window.set_workout_status(
                format!(
                    "Stage {} of {}: {} | {} remaining | {} W ({}% FTP)",
                    step_index + 1,
                    workout.steps.len(),
                    step.name,
                    format_duration(remaining_secs),
                    target_watts,
                    step.target_ftp_percent
                )
                .into(),
            );
            window.set_can_go_previous_stage(*step_index > 0);
            window.set_can_skip_stage(true);
        }
        WorkoutPhase::Finished => {
            set_timeline_stage_position(window, workout, workout.steps.len().checked_sub(1));
            window.set_target_power_watts(0);
            window.set_current_stage_name("Workout complete".into());
            window.set_current_stage_position(
                format!("{} STAGES COMPLETE", workout.steps.len()).into(),
            );
            window.set_step_remaining_text("00:00".into());
            window.set_current_target_percent(0);
            window.set_workout_status("Workout complete".into());
            window.set_can_go_previous_stage(!workout.steps.is_empty());
            window.set_can_skip_stage(false);
        }
    }
}

fn set_timeline_stage_position(window: &AppWindow, workout: &Workout, stage_index: Option<usize>) {
    let Some(stage_index) = stage_index else {
        window.set_timeline_active_stage_width(112);
        window.set_timeline_active_stage_offset(0);
        return;
    };

    let offset_px = timeline_stage_offset_px(workout, stage_index);
    let width_px = workout
        .steps
        .get(stage_index)
        .map(|step| timeline_stage_width_px(step.duration_secs))
        .unwrap_or(112);

    window.set_timeline_active_stage_width(i32::try_from(width_px).unwrap_or(i32::MAX));
    window.set_timeline_active_stage_offset(i32::try_from(offset_px).unwrap_or(i32::MAX));
}

fn timeline_stage_offset_px(workout: &Workout, stage_index: usize) -> u64 {
    workout
        .steps
        .iter()
        .take(stage_index)
        .fold(0_u64, |offset, step| {
            offset
                .saturating_add(timeline_stage_width_px(step.duration_secs))
                .saturating_add(8)
        })
}

fn timeline_stage_width_px(duration_secs: u64) -> u64 {
    duration_secs
        .saturating_mul(22)
        .saturating_add(99)
        .saturating_div(100)
        .max(112)
}

fn format_duration(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn default_workout_builder_configuration() -> WorkoutBuilderConfiguration {
    workout_builder_configuration(45, 3, 6, 6, 6, 6, 6)
}

fn legacy_workout_builder_configuration(
    length_minutes: i32,
    workout_type: i32,
    effort_intensity: i32,
    recovery_intensity: i32,
    effort_duration: i32,
    recovery_duration: i32,
    variation: i32,
) -> WorkoutBuilderConfiguration {
    workout_builder_configuration(
        length_minutes,
        workout_type,
        legacy_level(effort_intensity),
        legacy_level(recovery_intensity),
        legacy_level(effort_duration),
        legacy_level(recovery_duration),
        legacy_level(variation),
    )
}

fn workout_builder_configuration(
    length_minutes: i32,
    workout_type: i32,
    effort_intensity: i32,
    recovery_intensity: i32,
    effort_duration: i32,
    recovery_duration: i32,
    variation: i32,
) -> WorkoutBuilderConfiguration {
    WorkoutBuilderConfiguration {
        length_minutes: length_minutes.clamp(15, 180),
        workout_type: workout_type.clamp(0, 8),
        effort_intensity: effort_intensity.clamp(1, 11),
        recovery_intensity: recovery_intensity.clamp(1, 11),
        effort_duration: effort_duration.clamp(1, 11),
        recovery_duration: recovery_duration.clamp(1, 11),
        variation: variation.clamp(1, 11),
    }
}

fn legacy_level(level: i32) -> i32 {
    match level {
        i32::MIN..=0 => 1,
        1 => 3,
        2 => 6,
        3 => 9,
        _ => 11,
    }
}

fn workout_level(level: i32) -> u8 {
    u8::try_from(level.clamp(1, 11)).unwrap_or(6)
}

fn workout_type_from_level(level: i32) -> WorkoutType {
    match level {
        0 => WorkoutType::Recovery,
        1 => WorkoutType::Endurance,
        2 => WorkoutType::Tempo,
        4 => WorkoutType::Threshold,
        5 => WorkoutType::OverUnder,
        6 => WorkoutType::Vo2Max,
        7 => WorkoutType::Sprinting,
        8 => WorkoutType::Microbursts,
        _ => WorkoutType::SweetSpot,
    }
}

fn workout_from_stored(workout: &StoredWorkout) -> Workout {
    Workout::new(
        workout.name.clone(),
        workout
            .steps
            .iter()
            .map(|step| {
                indoor_cycling_core::WorkoutStep::new(
                    step.name.clone(),
                    step.duration_secs,
                    step.target_ftp_percent,
                )
            })
            .collect(),
    )
}

fn stored_workout(workout: &Workout) -> StoredWorkout {
    StoredWorkout {
        name: workout.name.clone(),
        steps: workout
            .steps
            .iter()
            .map(|step| StoredWorkoutStep {
                name: step.name.clone(),
                duration_secs: step.duration_secs,
                target_ftp_percent: step.target_ftp_percent,
            })
            .collect(),
    }
}

fn persist_configuration(store: Option<&ConfigurationStore>, state: &AppState) {
    let Some(store) = store else {
        return;
    };

    if let Err(error) = store.save(&state.configuration()) {
        eprintln!("Could not save application settings: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoor_cycling_core::{RideSample, WorkoutStep};

    #[test]
    fn replacing_workout_preserves_running_recording() {
        let sample = RideSample::new(1, 180, 88, Some(125));
        let mut state = AppState::new(None);
        state.recorder.push(sample.clone());
        state.running = true;
        state.workout.start();

        state.replace_workout(Workout::new(
            "Replacement",
            vec![WorkoutStep::new("New Effort", 60, 115)],
        ));

        assert_eq!(state.recorder.samples(), &[sample]);
        assert!(state.running);
        assert_eq!(state.workout.workout().name, "Replacement");
        assert!(matches!(
            state.workout.tick(),
            WorkoutPhase::Active { step, .. } if step.name == "New Effort"
        ));
    }

    #[test]
    fn replacing_workout_preserves_paused_recording() {
        let sample = RideSample::new(1, 160, 84, None);
        let mut state = AppState::new(None);
        state.recorder.push(sample.clone());
        state.running = false;

        state.replace_workout(Workout::new(
            "Paused Replacement",
            vec![WorkoutStep::new("New Warmup", 60, 55)],
        ));

        assert_eq!(state.recorder.samples(), &[sample]);
        assert!(!state.running);
        assert_eq!(state.workout.phase(), WorkoutPhase::NotStarted);
    }

    #[test]
    fn changing_ftp_rescales_an_active_workout_target() {
        let mut state = AppState::new(None);
        state.replace_workout(Workout::new(
            "FTP Scaling",
            vec![WorkoutStep::new("Effort", 60, 120)],
        ));
        state.running = true;
        state.workout.start();

        assert_eq!(current_target_power(&state), Some(240));

        state.ftp_watts = 250;
        apply_current_target(&mut state);

        assert_eq!(current_target_power(&state), Some(300));
        assert!(state.running);
    }

    #[test]
    fn timeline_offsets_follow_rendered_stage_widths() {
        let workout = Workout::new(
            "Timeline",
            vec![
                WorkoutStep::new("Short", 60, 50),
                WorkoutStep::new("Long", 900, 80),
                WorkoutStep::new("Next", 300, 100),
            ],
        );

        assert_eq!(timeline_stage_width_px(60), 112);
        assert_eq!(timeline_stage_width_px(900), 198);
        assert_eq!(timeline_stage_offset_px(&workout, 2), 326);
    }
}
