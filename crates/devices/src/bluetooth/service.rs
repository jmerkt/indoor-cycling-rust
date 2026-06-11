use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use btleplug::api::Peripheral as _;
use futures::{FutureExt, StreamExt};
use tokio::sync::mpsc::{self as tokio_mpsc, UnboundedReceiver, UnboundedSender};

use crate::bluetooth::connection::{self, DeviceConnection};
use crate::{
    BluetoothScanner, DeviceError, DeviceId, DeviceRole, DiscoveryCommand, DiscoveryEvent,
};

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

    pub fn connect(&self, role: DeviceRole, device_id: DeviceId) -> Result<(), DeviceError> {
        self.send(DiscoveryCommand::Connect { role, device_id })
    }

    pub fn disconnect(&self, role: DeviceRole) -> Result<(), DeviceError> {
        self.send(DiscoveryCommand::Disconnect { role })
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
    let mut scanner = BluetoothScanner::new();
    let mut connections = HashMap::<DeviceRole, DeviceConnection>::new();

    loop {
        if poll_notifications(&mut connections, &events) {
            disconnect_all(&mut connections, &events).await;
            return;
        }

        let command = if connections.is_empty() {
            commands.recv().await
        } else {
            tokio::select! {
                command = commands.recv() => command,
                _ = tokio::time::sleep(Duration::from_millis(20)) => continue,
            }
        };

        let Some(command) = command else {
            disconnect_all(&mut connections, &events).await;
            return;
        };

        if handle_command(command, &mut scanner, &mut connections, &events).await {
            return;
        }
    }
}

fn poll_notifications(
    connections: &mut HashMap<DeviceRole, DeviceConnection>,
    events: &std::sync::mpsc::Sender<DiscoveryEvent>,
) -> bool {
    let roles: Vec<_> = connections.keys().copied().collect();
    let mut disconnected = Vec::new();

    for role in roles {
        let Some(connection) = connections.get_mut(&role) else {
            continue;
        };
        let notification = connection
            .notifications
            .as_mut()
            .and_then(|notifications| notifications.next().now_or_never());

        match notification {
            Some(Some(notification)) => {
                if let Ok(Some(measurement)) = connection.parse_notification(&notification) {
                    if events
                        .send(DiscoveryEvent::Measurement { role, measurement })
                        .is_err()
                    {
                        return true;
                    }
                }
            }
            Some(None) => disconnected.push(role),
            None => {}
        }
    }

    for role in disconnected {
        if let Some(connection) = connections.remove(&role) {
            if events
                .send(DiscoveryEvent::RoleDisconnected {
                    role,
                    device_id: connection.device_id,
                })
                .is_err()
            {
                return true;
            }
        }
    }

    false
}

async fn handle_command(
    command: DiscoveryCommand,
    scanner: &mut BluetoothScanner,
    connections: &mut HashMap<DeviceRole, DeviceConnection>,
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
        DiscoveryCommand::Connect { role, device_id } => {
            disconnect_role(role, connections, events).await;
            if events
                .send(DiscoveryEvent::RoleConnecting {
                    role,
                    device_id: device_id.clone(),
                })
                .is_err()
            {
                return true;
            }

            let peripheral = match scanner.peripheral(&device_id) {
                Ok(peripheral) => peripheral,
                Err(error) => {
                    return events
                        .send(DiscoveryEvent::RoleError {
                            role,
                            message: error.to_string(),
                        })
                        .is_err();
                }
            };

            match connection::connect(role, device_id.clone(), peripheral).await {
                Ok(connection) => {
                    connections.insert(role, connection);
                    events
                        .send(DiscoveryEvent::RoleConnected { role, device_id })
                        .is_err()
                }
                Err(error) => events
                    .send(DiscoveryEvent::RoleError {
                        role,
                        message: error.to_string(),
                    })
                    .is_err(),
            }
        }
        DiscoveryCommand::Disconnect { role } => {
            disconnect_role(role, connections, events).await;
            false
        }
        DiscoveryCommand::Shutdown => {
            disconnect_all(connections, events).await;
            true
        }
    }
}

async fn disconnect_role(
    role: DeviceRole,
    connections: &mut HashMap<DeviceRole, DeviceConnection>,
    events: &std::sync::mpsc::Sender<DiscoveryEvent>,
) {
    let Some(connection) = connections.remove(&role) else {
        return;
    };

    let device_id = connection.device_id;
    let shared_by_another_role = connections
        .values()
        .any(|other| other.device_id == device_id);
    if !shared_by_another_role {
        let _ = connection.peripheral.disconnect().await;
    }

    let _ = events.send(DiscoveryEvent::RoleDisconnected { role, device_id });
}

async fn disconnect_all(
    connections: &mut HashMap<DeviceRole, DeviceConnection>,
    events: &std::sync::mpsc::Sender<DiscoveryEvent>,
) {
    let roles: Vec<_> = connections.keys().copied().collect();
    for role in roles {
        disconnect_role(role, connections, events).await;
    }
}
