use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use btleplug::api::Peripheral as _;
use futures::StreamExt;
use tokio::sync::mpsc::{self as tokio_mpsc, UnboundedReceiver, UnboundedSender};

use crate::bluetooth::heart_rate::{self, HeartRateConnection};
use crate::{BluetoothScanner, DeviceError, DeviceId, DiscoveryCommand, DiscoveryEvent};

pub struct BluetoothDeviceService {
    commands: UnboundedSender<DiscoveryCommand>,
    events: Receiver<DiscoveryEvent>,
    worker: Option<JoinHandle<()>>,
}

impl BluetoothDeviceService {
    pub fn start() -> Result<Self, DeviceError> {
        let (command_sender, command_receiver) = tokio_mpsc::unbounded_channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("bluetooth-device-service".to_owned())
            .spawn(move || run_worker(command_receiver, event_sender))
            .map_err(|error| DeviceError::BluetoothUnavailable(error.to_string()))?;

        Ok(Self {
            commands: command_sender,
            events: event_receiver,
            worker: Some(worker),
        })
    }

    pub fn scan(&self, duration: Duration) -> Result<(), DeviceError> {
        self.send(DiscoveryCommand::Scan { duration })
    }

    pub fn connect_heart_rate(&self, device_id: DeviceId) -> Result<(), DeviceError> {
        self.send(DiscoveryCommand::ConnectHeartRate { device_id })
    }

    pub fn disconnect_heart_rate(&self) -> Result<(), DeviceError> {
        self.send(DiscoveryCommand::DisconnectHeartRate)
    }

    pub fn try_recv(&self) -> Result<DiscoveryEvent, mpsc::TryRecvError> {
        self.events.try_recv()
    }

    pub fn recv(&self) -> Result<DiscoveryEvent, DeviceError> {
        self.events.recv().map_err(|_| DeviceError::ServiceStopped)
    }

    fn send(&self, command: DiscoveryCommand) -> Result<(), DeviceError> {
        self.commands
            .send(command)
            .map_err(|_| DeviceError::ServiceStopped)
    }
}

impl Drop for BluetoothDeviceService {
    fn drop(&mut self) {
        let _ = self.commands.send(DiscoveryCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_worker(
    commands: UnboundedReceiver<DiscoveryCommand>,
    events: std::sync::mpsc::Sender<DiscoveryEvent>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = events.send(DiscoveryEvent::Error(format!(
                "failed to start Bluetooth runtime: {error}"
            )));
            return;
        }
    };

    runtime.block_on(run_service(commands, events));
}

async fn run_service(
    mut commands: UnboundedReceiver<DiscoveryCommand>,
    events: std::sync::mpsc::Sender<DiscoveryEvent>,
) {
    let scanner = BluetoothScanner::new();
    let mut heart_rate_connection: Option<HeartRateConnection> = None;

    loop {
        if let Some(connection) = heart_rate_connection.as_mut() {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else {
                        disconnect(&mut heart_rate_connection, &events).await;
                        return;
                    };
                    if handle_command(
                        command,
                        &scanner,
                        &mut heart_rate_connection,
                        &events,
                    ).await {
                        return;
                    }
                }
                notification = connection.notifications.next() => {
                    match notification {
                        Some(notification) if notification.uuid == crate::bluetooth::uuids::HEART_RATE_MEASUREMENT => {
                            if let Ok(value) = heart_rate::parse_measurement(&notification.value) {
                                if events.send(DiscoveryEvent::HeartRateMeasurement(value)).is_err() {
                                    return;
                                }
                            }
                        }
                        Some(_) => {}
                        None => {
                            let device_id = connection.device_id.clone();
                            heart_rate_connection = None;
                            if events.send(DiscoveryEvent::HeartRateDisconnected(device_id)).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        } else {
            let Some(command) = commands.recv().await else {
                return;
            };
            if handle_command(command, &scanner, &mut heart_rate_connection, &events).await {
                return;
            }
        }
    }
}

async fn handle_command(
    command: DiscoveryCommand,
    scanner: &BluetoothScanner,
    heart_rate_connection: &mut Option<HeartRateConnection>,
    events: &std::sync::mpsc::Sender<DiscoveryEvent>,
) -> bool {
    match command {
        DiscoveryCommand::Scan { duration } => {
            if events.send(DiscoveryEvent::ScanStarted).is_err() {
                return true;
            }

            match scanner.scan(duration).await {
                Ok(devices) => {
                    for device in devices {
                        if events
                            .send(DiscoveryEvent::DeviceDiscovered(device))
                            .is_err()
                        {
                            return true;
                        }
                    }
                }
                Err(error) => {
                    if events
                        .send(DiscoveryEvent::Error(error.to_string()))
                        .is_err()
                    {
                        return true;
                    }
                }
            }

            events.send(DiscoveryEvent::ScanFinished).is_err()
        }
        DiscoveryCommand::ConnectHeartRate { device_id } => {
            disconnect(heart_rate_connection, events).await;
            if events
                .send(DiscoveryEvent::HeartRateConnecting(device_id.clone()))
                .is_err()
            {
                return true;
            }

            match heart_rate::connect(device_id.clone()).await {
                Ok(connection) => {
                    *heart_rate_connection = Some(connection);
                    events
                        .send(DiscoveryEvent::HeartRateConnected(device_id))
                        .is_err()
                }
                Err(error) => events
                    .send(DiscoveryEvent::HeartRateError(format!(
                        "heart rate connection failed: {error}"
                    )))
                    .is_err(),
            }
        }
        DiscoveryCommand::DisconnectHeartRate => {
            disconnect(heart_rate_connection, events).await;
            false
        }
        DiscoveryCommand::Shutdown => {
            disconnect(heart_rate_connection, events).await;
            true
        }
    }
}

async fn disconnect(
    connection: &mut Option<HeartRateConnection>,
    events: &std::sync::mpsc::Sender<DiscoveryEvent>,
) {
    let Some(connection) = connection.take() else {
        return;
    };

    let device_id = connection.device_id;
    let _ = connection.peripheral.disconnect().await;
    let _ = events.send(DiscoveryEvent::HeartRateDisconnected(device_id));
}
