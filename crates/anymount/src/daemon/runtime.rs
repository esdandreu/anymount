use crate::daemon::messages::DaemonMessage;
use crate::logger::Logger;
use std::sync::mpsc::Receiver;

pub struct DaemonRuntime<L: Logger> {
    logger: L,
    rx: Receiver<DaemonMessage>,
}

impl<L: Logger> DaemonRuntime<L> {
    pub fn new(logger: L, rx: Receiver<DaemonMessage>) -> Self {
        Self { logger, rx }
    }

    pub fn run(&mut self) -> Result<(), String> {
        loop {
            match self.rx.recv().map_err(|error| error.to_string())? {
                DaemonMessage::Telemetry(message) => self.logger.info(message),
                DaemonMessage::Shutdown => break,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DaemonRuntime;
    use crate::daemon::messages::DaemonMessage;
    use crate::logger::Logger;
    use std::fmt::Display;
    use std::sync::{Arc, Mutex, mpsc};

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
    fn daemon_logs_telemetry_until_shutdown() {
        let (tx, rx) = mpsc::channel();
        let logger = RecordingLogger::default();
        let mut runtime = DaemonRuntime::new(logger.clone(), rx);

        tx.send(DaemonMessage::Telemetry("opened: file.txt".into()))
            .expect("send should work");
        tx.send(DaemonMessage::Shutdown).expect("send should work");

        runtime.run().expect("runtime should succeed");
        assert_eq!(logger.entries(), vec!["opened: file.txt"]);
    }
}
