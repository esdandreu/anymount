use crate::logger::Logger;
use crate::service::control::messages::ServiceMessage;
use crate::service::Result;
use std::sync::mpsc::Receiver;

pub struct ServiceRuntime<L: Logger> {
    logger: L,
    rx: Receiver<ServiceMessage>,
}

impl<L: Logger> ServiceRuntime<L> {
    pub fn new(logger: L, rx: Receiver<ServiceMessage>) -> Self {
        Self { logger, rx }
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            match self.rx.recv()? {
                ServiceMessage::Telemetry(message) => self.logger.info(message),
                ServiceMessage::Shutdown => break,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceRuntime;
    use crate::logger::Logger;
    use crate::service::control::messages::ServiceMessage;
    use std::fmt::Display;
    use std::sync::{mpsc, Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingLogger {
        entries: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingLogger {
        fn entries(&self) -> Vec<String> {
            self.entries
                .lock()
                .expect("logger should not be poisoned")
                .clone()
        }
    }

    impl Logger for RecordingLogger {
        fn trace(&self, _msg: impl Display) {}
        fn debug(&self, _msg: impl Display) {}
        fn info(&self, msg: impl Display) {
            self.entries
                .lock()
                .expect("logger should not be poisoned")
                .push(msg.to_string());
        }
        fn warn(&self, _msg: impl Display) {}
        fn error(&self, _msg: impl Display) {}
    }

    #[test]
    fn service_runtime_logs_telemetry_until_shutdown() {
        let (tx, rx) = mpsc::channel();
        let logger = RecordingLogger::default();
        let mut runtime = ServiceRuntime::new(logger.clone(), rx);

        tx.send(ServiceMessage::Telemetry("opened: file.txt".into()))
            .expect("send should work");
        tx.send(ServiceMessage::Shutdown).expect("send should work");

        runtime.run().expect("runtime should succeed");
        assert_eq!(logger.entries(), vec!["opened: file.txt"]);
    }
}
