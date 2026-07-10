use std::{collections::BTreeMap, fmt::Debug, future::Future, sync::Arc};

use serde::Serialize;
use winnow::Parser;

mod info;
mod response;

use response::response;
pub use response::Response;

use print3rs_serializer::{serialize_unsequenced, Sequenced};

use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt},
    sync::{broadcast, mpsc, oneshot},
    task::JoinHandle,
};

pub type LineStream = broadcast::Receiver<Arc<str>>;

#[derive(Debug)]
struct SendContent {
    content: Box<[u8]>,
    sequence: Option<i32>,
    responder: Option<oneshot::Sender<()>>,
}

impl SendContent {
    const fn new(
        content: Box<[u8]>,
        sequence: Option<i32>,
        responder: Option<oneshot::Sender<()>>,
    ) -> Self {
        Self {
            content,
            sequence,
            responder,
        }
    }
}

impl From<(Box<[u8]>, Option<i32>, Option<oneshot::Sender<()>>)> for SendContent {
    fn from(value: (Box<[u8]>, Option<i32>, Option<oneshot::Sender<()>>)) -> Self {
        SendContent::new(value.0, value.1, value.2)
    }
}

#[derive(Debug)]
pub struct Socket {
    sender: mpsc::Sender<SendContent>,
    serializer: Sequenced,
    pub responses: broadcast::Receiver<Arc<str>>,
}

impl Clone for Socket {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            serializer: self.serializer.clone(),
            responses: self.responses.resubscribe(),
        }
    }
}

impl Socket {
    /// Serialize a struct implementing Serialize and send the bytes to the printer
    ///
    /// Sent bytes will include a sequence number and checksum.
    /// For printers which support advanced OK messages this will allow TCP like checked communication.
    ///
    /// When called, a local task is spawned to check for a matching OK message.
    /// The handle to this task is returned after the first await on success.
    /// This allows simple synchronization of any sent command by awaiting twice.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn send(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        let send_slot = self.sender.reserve().await?;
        let (sequence, bytes) = self.serializer.serialize(gcode);
        let (responder, response) = oneshot::channel();
        send_slot.send(SendContent::new(bytes, Some(sequence), Some(responder)));
        let response = async { response.await.map_err(|_| Error::WontRespond) };
        Ok(response)
    }

    /// Serialize and attempt sending payload to connected device.
    ///
    /// Non-blocking non-async implementation, returns with an error if a wait would occur
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn try_send(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        let send_slot = self.sender.try_reserve()?;
        let (sequence, bytes) = self.serializer.serialize(gcode);
        let (responder, response) = oneshot::channel();
        send_slot.send(SendContent::new(bytes, Some(sequence), Some(responder)));
        let response = async { response.await.map_err(|_| Error::WontRespond) };
        Ok(response)
    }

    /// Serialize anything implementing Serialize and send the bytes to the printer
    ///
    /// There is no guarantee that a command is correctly recieved or serviced;
    /// any synchronization based on responses will have to be done manually.
    ///
    /// If your printer supports it, the sequenced `send` function is preferred,
    /// although this version is slightly lower overhead.
    pub async fn send_unsequenced(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        let bytes = serialize_unsequenced(gcode);
        let (responder, response) = oneshot::channel();
        let send_slot = self.sender.reserve().await?;
        send_slot.send(SendContent::new(bytes, None, Some(responder)));
        let response = async { response.await.map_err(|_| Error::WontRespond) };
        Ok(response)
    }

    /// Non-blocking non-async version of `send_unsequenced`, see that method for usage
    ///
    /// Where `send_unsequenced` would wait, this method returns an error
    pub fn try_send_unsequenced(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        let bytes = serialize_unsequenced(gcode);
        let (responder, response) = oneshot::channel();
        let send_slot = self.sender.try_reserve()?;
        send_slot.send(SendContent::new(bytes, None, Some(responder)));
        let response = async { response.await.map_err(|_| Error::WontRespond) };
        Ok(response)
    }

    /// Send any raw sequence of bytes to the printer
    pub async fn send_raw(&self, gcode: &[u8]) -> Result<(), Error> {
        let sender = self.sender.reserve().await?;
        sender.send(SendContent::new(
            gcode.to_owned().into_boxed_slice(),
            None,
            None,
        ));
        Ok(())
    }

    /// Send any raw sequence of bytes to the printer
    pub fn try_send_raw(&self, gcode: &[u8]) -> Result<(), Error> {
        let sender = self.sender.try_reserve()?;
        sender.send(SendContent::new(
            gcode.to_owned().into_boxed_slice(),
            None,
            None,
        ));
        Ok(())
    }

    /// Read the next line from the printer
    ///
    /// May not recieve all lines, if calls to this function are spaced
    /// far apart, the buffer may overfill and the oldest messages will
    /// be dropped. In this case the oldest available message is returned.
    pub async fn read_next_line(&mut self) -> Result<Arc<str>, Error> {
        let line = self.responses.recv().await?;
        Ok(line)
    }

    /// See `read_next_line`
    ///
    /// Where that method would await, this will return immediately with an error.
    pub fn try_read_next_line(&mut self) -> Result<Arc<str>, Error> {
        let line = self.responses.try_recv()?;
        Ok(line)
    }

    /// Obtain a broadcast receiver returning all lines received by the printer
    pub fn subscribe_lines(&self) -> Result<LineStream, Error> {
        Ok(self.responses.resubscribe())
    }
}

/// Handle for asynchronous serial communication with a 3D printer
#[derive(Debug, Default)]
pub enum Printer {
    #[default]
    Disconnected,
    Connected {
        socket: Socket,
        com_task: tokio::task::JoinHandle<()>,
    },
}

impl Drop for Printer {
    fn drop(&mut self) {
        if let Self::Connected { com_task, .. } = self {
            com_task.abort()
        }
    }
}

impl PartialEq for Printer {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Printer sent a bad message")]
    ResponseSender(#[from] broadcast::error::SendError<Arc<str>>),

    #[error("Failed to send command, try again")]
    Sender(#[from] mpsc::error::TrySendError<()>),

    #[error("Failed to send command, printer may have disconnected")]
    SendReserve(#[from] mpsc::error::SendError<()>),

    #[error("Not connected to a printer")]
    Disconnected,

    #[error("Ok not received")]
    WontRespond,

    #[error("No responses recieved, try again")]
    TryReadLine(#[from] broadcast::error::TryRecvError),

    #[error("No responses received, printer may have disconnected")]
    ReadLine(#[from] broadcast::error::RecvError),
}

/// Loop for handling sending/receiving in the background with possible split senders/receivers
async fn printer_com_task(
    mut transport: impl AsyncBufRead + AsyncWrite + Unpin,
    mut gcoderx: mpsc::Receiver<SendContent>,
    responsetx: broadcast::Sender<Arc<str>>,
) {
    tracing::debug!("Started background printer communications");
    let mut buf = String::new();
    let mut pending_responses = BTreeMap::new();
    loop {
        tokio::select! {
            Some(SendContent{content, sequence, responder}) = gcoderx.recv(), if pending_responses.len() < 4 => {
                if transport.write_all(&content).await.is_err() {return;}
                if transport.flush().await.is_err() {return;}
                tracing::debug!("Sent `{}` to printer", String::from_utf8_lossy(&content).trim());
                if let Some(responder) = responder {
                    // dropping anything in slot, gives WontRespond error
                    pending_responses.insert(sequence, (responder, content));
                }
            },
            Ok(1..) = transport.read_line(&mut buf) => {
                tracing::debug!("Received `{buf}` from printer");
                if let Ok(ok_res) = response.parse(buf.as_bytes()) {
                    match ok_res {
                        Response::Ok(ref maybe_seq) => {
                            if let Some((responder, _)) = pending_responses.remove(maybe_seq){
                                 let _ = responder.send(());
                            }
                        },
                        Response::Resend(ref maybe_seq) => {
                            if let Some((_, line)) = pending_responses.get(maybe_seq) {
                                if transport.write_all(line).await.is_err() {return;}
                                if transport.flush().await.is_err() {return;}
                                tracing::debug!("Resent `{}` to printer", String::from_utf8_lossy(line).trim());
                            }
                        },
                    }
                }
                if responsetx.send(Arc::from(buf.split_off(0))).is_err() {return;}
            },
            else => return,
        }
    }
}

impl Printer {
    /// Create a new printer from a SerialStream.
    ///
    /// Starts a local task to handle printer communication asynchronously
    #[tracing::instrument(level = "debug")]
    pub fn new<S>(port: S) -> Self
    where
        S: AsyncBufRead + AsyncWrite + Unpin + Send + 'static + Debug,
    {
        let (sender, gcoderx) = mpsc::channel::<SendContent>(16);
        let (response_sender, responses) = broadcast::channel(64);
        let com_task = tokio::task::spawn(printer_com_task(port, gcoderx, response_sender));
        let serializer = Sequenced::default();
        Self::Connected {
            socket: Socket {
                sender,
                serializer,
                responses,
            },
            com_task,
        }
    }

    /// Connect to a device
    pub fn connect<S>(&mut self, port: S)
    where
        S: AsyncBufRead + AsyncWrite + Unpin + Send + 'static + Debug,
    {
        *self = Printer::new(port);
    }

    /// Obtain a cloneable socket handle to talk to printer
    pub fn socket(&self) -> Result<&Socket, Error> {
        match self {
            Self::Disconnected => Err(Error::Disconnected),
            Self::Connected { socket, .. } => Ok(socket),
        }
    }

    /// Obtain an exclusive socket handle - needed to read
    pub fn socket_mut(&mut self) -> Result<&mut Socket, Error> {
        match self {
            Self::Disconnected => Err(Error::Disconnected),
            Self::Connected { socket, .. } => Ok(socket),
        }
    }

    /// Disconnect the printer and shutdown background communication
    pub fn disconnect(&mut self) {
        core::mem::take(self);
    }

    /// Check if there is an active connection, convenience method for testing enum state.
    pub fn is_connected(&self) -> bool {
        match self {
            Printer::Disconnected => false,
            Printer::Connected { .. } => true,
        }
    }

    /// Get a handle to background processing task if one is active.
    pub fn background_task(&self) -> Option<&JoinHandle<()>> {
        match self {
            Printer::Disconnected => None,
            Printer::Connected { com_task, .. } => Some(com_task),
        }
    }

    /// Serialize a struct implementing Serialize and send the bytes to the printer
    ///
    /// Sent bytes will include a sequence number and checksum.
    /// For printers which support advanced OK messages this will allow TCP like checked communication.
    ///
    /// When called, a local task is spawned to check for a matching OK message.
    /// The handle to this task is returned after the first await on success.
    /// This allows simple synchronization of any sent command by awaiting twice.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn send(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        self.socket()?.send(gcode).await
    }

    /// Non blocking, non-async version of `send`, instantly returns an error where that method would wait
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn try_send(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        self.socket()?.try_send(gcode)
    }

    /// Serialize anything implementing Serialize and send the bytes to the printer
    ///
    /// There is no guarantee that a command is correctly recieved or serviced;
    /// any synchronization based on responses will have to be done manually.
    ///
    /// If your printer supports it, the sequenced `send` function is preferred,
    /// although this version is slightly lower overhead.
    pub async fn send_unsequenced(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        self.socket()?.send_unsequenced(gcode).await
    }

    /// Non blocking, non-async version of `send_unsequenced`, instantly returns an error where that method would wait
    pub fn try_send_unsequenced(
        &self,
        gcode: impl Serialize + Debug,
    ) -> Result<impl Future<Output = Result<(), Error>>, Error> {
        self.socket()?.try_send_unsequenced(gcode)
    }

    /// Send any raw sequence of bytes to the printer
    pub async fn send_raw(&self, gcode: &[u8]) -> Result<(), Error> {
        self.socket()?.send_raw(gcode).await
    }

    /// Non blocking, non-async version of `send_raw`, instantly returns an error where that method would wait
    pub fn try_send_raw(&self, gcode: &[u8]) -> Result<(), Error> {
        self.socket()?.try_send_raw(gcode)
    }

    /// Read the next line from the printer
    ///
    /// May not recieve all lines, if calls to this function are spaced
    /// far apart, the buffer may overfill and the oldest messages will
    /// be dropped. In this case the oldest available message is returned.
    pub async fn read_next_line(&mut self) -> Result<Arc<str>, Error> {
        self.socket_mut()?.read_next_line().await
    }

    /// Non blocking, non-async version of `read_next_line`, instantly returns an error where that method would wait
    pub fn try_read_next_line(&mut self) -> Result<Arc<str>, Error> {
        self.socket_mut()?.try_read_next_line()
    }

    /// Obtain a broadcast receiver returning all lines received by the printer
    pub fn subscribe_lines(&self) -> Result<LineStream, Error> {
        self.socket()?.subscribe_lines()
    }
}

impl From<Option<Printer>> for Printer {
    fn from(value: Option<Printer>) -> Self {
        match value {
            Some(printer) => printer,
            None => Printer::Disconnected,
        }
    }
}

impl From<&Printer> for Option<Socket> {
    fn from(value: &Printer) -> Self {
        value.socket().ok().cloned()
    }
}

impl<'a> From<&'a Printer> for Option<&'a Socket> {
    fn from(value: &'a Printer) -> Self {
        value.socket().ok()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn disconnected_is_disconnected() {
        let mut disconnected = Printer::Disconnected;

        assert!(matches!(
            disconnected.try_send(()),
            Err(Error::Disconnected)
        ));

        assert!(matches!(
            disconnected.try_send_raw(b""),
            Err(Error::Disconnected)
        ));

        assert!(matches!(
            disconnected.try_read_next_line(),
            Err(Error::Disconnected)
        ));

        assert!(matches!(disconnected.socket(), Err(Error::Disconnected)));
    }

    #[test]
    fn conversion() {
        let disconnected: Printer = None.into();
        assert!(!disconnected.is_connected());
        let maybe_socket: Option<&Socket> = (&disconnected).into();
        assert!(maybe_socket.is_none());
    }
}
