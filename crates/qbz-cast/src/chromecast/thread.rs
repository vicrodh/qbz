//! Chromecast connection handler running in a dedicated thread.
//!
//! Since rust_cast uses Rc (not Arc), it cannot be shared across threads.
//! This module provides a thread-safe wrapper using channels.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::chromecast::device::CastDeviceConnection;
use crate::CastError;
use crate::chromecast::{CastStatus, MediaMetadata, CastPositionInfo};

/// Commands sent to the Chromecast thread
pub enum CastCommand {
    Connect {
        ip: String,
        port: u16,
        reply: Sender<Result<(), CastError>>,
    },
    Disconnect {
        reply: Sender<Result<(), CastError>>,
    },
    GetStatus {
        reply: Sender<Result<CastStatus, CastError>>,
    },
    GetMediaPosition {
        reply: Sender<Result<CastPositionInfo, CastError>>,
    },
    LoadMedia {
        url: String,
        content_type: String,
        metadata: MediaMetadata,
        reply: Sender<Result<(), CastError>>,
    },
    Play {
        reply: Sender<Result<(), CastError>>,
    },
    Pause {
        reply: Sender<Result<(), CastError>>,
    },
    Stop {
        reply: Sender<Result<(), CastError>>,
    },
    SetVolume {
        volume: f32,
        reply: Sender<Result<(), CastError>>,
    },
    Seek {
        position_secs: f64,
        reply: Sender<Result<(), CastError>>,
    },
    Shutdown,
}

/// Thread-safe handle to communicate with the Chromecast thread
pub struct ChromecastHandle {
    sender: Sender<CastCommand>,
    _thread: JoinHandle<()>,
}

impl ChromecastHandle {
    /// Start the Chromecast handler thread
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            chromecast_thread_main(receiver);
        });

        Self {
            sender,
            _thread: thread,
        }
    }

    /// Connect to a Chromecast device
    pub fn connect(&self, ip: String, port: u16) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::Connect {
                ip,
                port,
                reply: reply_tx,
            })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Disconnect from the current device
    pub fn disconnect(&self) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::Disconnect { reply: reply_tx })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Get device status
    pub fn get_status(&self) -> Result<CastStatus, CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::GetStatus { reply: reply_tx })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Load media for playback
    pub fn load_media(
        &self,
        url: String,
        content_type: String,
        metadata: MediaMetadata,
    ) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::LoadMedia {
                url,
                content_type,
                metadata,
                reply: reply_tx,
            })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Play
    pub fn play(&self) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::Play { reply: reply_tx })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Pause
    pub fn pause(&self) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::Pause { reply: reply_tx })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Stop
    pub fn stop(&self) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::Stop { reply: reply_tx })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Set volume
    pub fn set_volume(&self, volume: f32) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::SetVolume {
                volume,
                reply: reply_tx,
            })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Seek
    pub fn seek(&self, position_secs: f64) -> Result<(), CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::Seek {
                position_secs,
                reply: reply_tx,
            })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }

    /// Get media position for seekbar updates
    pub fn get_media_position(&self) -> Result<CastPositionInfo, CastError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(CastCommand::GetMediaPosition { reply: reply_tx })
            .map_err(|_| CastError::Connection("Thread communication error".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| CastError::Connection("Thread response error".to_string()))?
    }
}

impl Default for ChromecastHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ChromecastHandle {
    fn drop(&mut self) {
        let _ = self.sender.send(CastCommand::Shutdown);
    }
}

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);

/// Main loop for the Chromecast thread
fn chromecast_thread_main(receiver: Receiver<CastCommand>) {
    let mut connection: Option<CastDeviceConnection> = None;

    loop {
        let command = match receiver.recv_timeout(HEARTBEAT_INTERVAL) {
            Ok(cmd) => cmd,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(conn) = connection.as_ref() {
                    if let Err(err) = conn.heartbeat() {
                        log::warn!("Chromecast heartbeat failed: {}", err);
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break, // Channel closed
        };

        match command {
            CastCommand::Connect { ip, port, reply } => {
                let result = CastDeviceConnection::connect(&ip, port);
                match result {
                    Ok(conn) => {
                        connection = Some(conn);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }

            CastCommand::Disconnect { reply } => {
                let result = if let Some(ref mut conn) = connection {
                    conn.disconnect()
                } else {
                    Ok(())
                };
                connection = None;
                let _ = reply.send(result);
            }

            CastCommand::GetStatus { reply } => {
                let result = match connection.as_ref() {
                    Some(conn) => conn.get_status(),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::GetMediaPosition { reply } => {
                let result = match connection.as_mut() {
                    Some(conn) => conn.get_media_position(),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::LoadMedia {
                url,
                content_type,
                metadata,
                reply,
            } => {
                let result = match connection.as_mut() {
                    Some(conn) => conn.load_media(&url, &content_type, metadata),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::Play { reply } => {
                let result = match connection.as_mut() {
                    Some(conn) => conn.play(),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::Pause { reply } => {
                let result = match connection.as_mut() {
                    Some(conn) => conn.pause(),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::Stop { reply } => {
                let result = match connection.as_mut() {
                    Some(conn) => conn.stop(),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::SetVolume { volume, reply } => {
                let result = match connection.as_mut() {
                    Some(conn) => conn.set_volume(volume),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::Seek {
                position_secs,
                reply,
            } => {
                let result = match connection.as_mut() {
                    Some(conn) => conn.seek(position_secs),
                    None => Err(CastError::NotConnected),
                };
                let _ = reply.send(result);
            }

            CastCommand::Shutdown => {
                if let Some(mut conn) = connection.take() {
                    let _ = conn.disconnect();
                }
                break;
            }
        }
    }
}
