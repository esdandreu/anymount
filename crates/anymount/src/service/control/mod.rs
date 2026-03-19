pub mod messages;
pub mod paths;

#[cfg(unix)]
pub mod unix;

#[cfg(target_os = "windows")]
pub mod windows;

use self::messages::ControlMessage;
use crate::service::{Error, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub trait ControlTransport {
    type Server;

    fn bind(&self, provider_name: &str) -> Result<Self::Server>;

    fn send(&self, provider_name: &str, message: ControlMessage) -> Result<ControlMessage>;
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryControlTransport {
    responses: Arc<Mutex<HashMap<String, Vec<ControlMessage>>>>,
}

impl InMemoryControlTransport {
    pub fn serve_once<F>(&self, server: InMemoryServer, handler: F) -> Result<()>
    where
        F: FnOnce(ControlMessage) -> ControlMessage,
    {
        let mut responses = self.responses.lock().map_err(|_| Error::Poisoned)?;
        responses.insert(server.provider_name, vec![handler(ControlMessage::Ping)]);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct InMemoryServer {
    provider_name: String,
}

impl ControlTransport for InMemoryControlTransport {
    type Server = InMemoryServer;

    fn bind(&self, provider_name: &str) -> Result<Self::Server> {
        Ok(InMemoryServer {
            provider_name: provider_name.to_owned(),
        })
    }

    fn send(&self, provider_name: &str, _message: ControlMessage) -> Result<ControlMessage> {
        let mut responses = self.responses.lock().map_err(|_| Error::Poisoned)?;
        let queue = responses
            .get_mut(provider_name)
            .ok_or_else(|| Error::NotBound {
                provider_name: provider_name.to_owned(),
            })?;
        if queue.is_empty() {
            return Err(Error::NoQueuedResponse {
                provider_name: provider_name.to_owned(),
            });
        }
        Ok(queue.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlTransport, InMemoryControlTransport};
    use crate::service::control::messages::ControlMessage;

    #[test]
    fn client_ping_receives_ready() {
        let transport = InMemoryControlTransport::default();
        let server = transport.bind("demo").expect("bind should succeed");
        transport
            .serve_once(server, |message| match message {
                ControlMessage::Ping => ControlMessage::Ready,
                other => ControlMessage::Error(format!("unexpected: {other:?}")),
            })
            .expect("serve should succeed");

        let reply = transport
            .send("demo", ControlMessage::Ping)
            .expect("send should succeed");
        assert_eq!(reply, ControlMessage::Ready);
    }
}
