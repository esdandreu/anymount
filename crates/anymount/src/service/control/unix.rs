use crate::service::control::messages::ControlMessage;
use crate::service::control::paths::provider_endpoint;
use crate::service::{Error, Result};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};

#[derive(Debug, Default, Clone, Copy)]
pub struct UnixControl;

impl UnixControl {
    pub fn bind(&self, provider_name: &str) -> Result<UnixListener> {
        let path = provider_endpoint(provider_name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                operation: "create unix socket directory",
                provider_name: provider_name.to_owned(),
                source,
            })?;
        }
        if path.exists() {
            std::fs::remove_file(&path).map_err(|source| Error::Io {
                operation: "remove stale unix socket",
                provider_name: provider_name.to_owned(),
                source,
            })?;
        }
        UnixListener::bind(path).map_err(|source| Error::Io {
            operation: "bind unix socket",
            provider_name: provider_name.to_owned(),
            source,
        })
    }

    pub fn send(&self, provider_name: &str, message: ControlMessage) -> Result<ControlMessage> {
        let path = provider_endpoint(provider_name)?;
        let mut stream = UnixStream::connect(path).map_err(|source| Error::Io {
            operation: "connect unix socket",
            provider_name: provider_name.to_owned(),
            source,
        })?;
        stream
            .write_all(&message.encode())
            .map_err(|source| Error::Io {
                operation: "write unix socket message",
                provider_name: provider_name.to_owned(),
                source,
            })?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|source| Error::Io {
                operation: "shutdown unix socket writer",
                provider_name: provider_name.to_owned(),
                source,
            })?;

        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).map_err(|source| Error::Io {
            operation: "read unix socket reply",
            provider_name: provider_name.to_owned(),
            source,
        })?;
        ControlMessage::decode(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::UnixControl;
    use crate::service::control::messages::ControlMessage;
    use std::io::{Read, Write};

    #[test]
    fn unix_control_send_round_trips_ping() {
        let control = UnixControl;
        let listener = control.bind("unix-roundtrip").expect("bind should succeed");

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept should succeed");
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).expect("read should succeed");
            assert_eq!(
                ControlMessage::decode(&bytes).expect("decode should succeed"),
                ControlMessage::Ping
            );
            stream
                .write_all(&ControlMessage::Ready.encode())
                .expect("write should succeed");
        });

        let reply = control
            .send("unix-roundtrip", ControlMessage::Ping)
            .expect("send should succeed");
        assert_eq!(reply, ControlMessage::Ready);

        server.join().expect("server thread should finish");
    }

    #[test]
    fn unix_control_send_shutdown_receives_ack() {
        let control = UnixControl;
        let listener = control.bind("unix-shutdown").expect("bind should succeed");

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept should succeed");
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).expect("read should succeed");
            assert_eq!(
                ControlMessage::decode(&bytes).expect("decode should succeed"),
                ControlMessage::Shutdown
            );
            stream
                .write_all(&ControlMessage::Ack.encode())
                .expect("write should succeed");
        });

        let reply = control
            .send("unix-shutdown", ControlMessage::Shutdown)
            .expect("send should succeed");
        assert_eq!(reply, ControlMessage::Ack);

        server.join().expect("server thread should finish");
    }
}
