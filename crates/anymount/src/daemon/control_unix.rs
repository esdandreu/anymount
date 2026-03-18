use crate::daemon::messages::ControlMessage;
use crate::daemon::paths::provider_endpoint;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};

#[derive(Debug, Default, Clone, Copy)]
pub struct UnixControl;

impl UnixControl {
    pub fn bind(&self, provider_name: &str) -> Result<UnixListener, String> {
        let path = provider_endpoint(provider_name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create unix socket directory: {error}"))?;
        }
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|error| format!("remove stale unix socket: {error}"))?;
        }
        UnixListener::bind(path).map_err(|error| format!("bind unix socket: {error}"))
    }

    pub fn send(
        &self,
        provider_name: &str,
        message: ControlMessage,
    ) -> Result<ControlMessage, String> {
        let path = provider_endpoint(provider_name)?;
        let mut stream =
            UnixStream::connect(path).map_err(|error| format!("connect unix socket: {error}"))?;
        stream
            .write_all(&message.encode())
            .map_err(|error| format!("write unix socket message: {error}"))?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|error| format!("shutdown unix socket writer: {error}"))?;

        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .map_err(|error| format!("read unix socket reply: {error}"))?;
        ControlMessage::decode(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::UnixControl;
    use crate::daemon::messages::ControlMessage;
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
}
