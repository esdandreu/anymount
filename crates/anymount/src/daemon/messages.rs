use crate::daemon::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMessage {
    Ping,
    Ready,
    Shutdown,
    Ack,
    Error(String),
}

impl ControlMessage {
    pub fn encode(&self) -> Vec<u8> {
        let value = match self {
            Self::Ping => "ping".to_owned(),
            Self::Ready => "ready".to_owned(),
            Self::Shutdown => "shutdown".to_owned(),
            Self::Ack => "ack".to_owned(),
            Self::Error(message) => format!("error:{message}"),
        };
        value.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let value = std::str::from_utf8(bytes)?;

        match value {
            "ping" => Ok(Self::Ping),
            "ready" => Ok(Self::Ready),
            "shutdown" => Ok(Self::Shutdown),
            "ack" => Ok(Self::Ack),
            _ => value
                .strip_prefix("error:")
                .map(|message| Self::Error(message.to_owned()))
                .ok_or_else(|| Error::UnknownControlMessage {
                    value: value.to_owned(),
                }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonMessage {
    Shutdown,
    Telemetry(String),
}

#[cfg(test)]
mod tests {
    use super::ControlMessage;
    use crate::daemon::Error;

    #[test]
    fn control_message_round_trips() {
        let encoded = ControlMessage::Ping.encode();
        let decoded = ControlMessage::decode(&encoded).expect("decode should work");
        assert_eq!(decoded, ControlMessage::Ping);
    }

    #[test]
    fn decode_invalid_utf8_returns_decode_error() {
        let err = ControlMessage::decode(&[0xff]).expect_err("decode should fail");
        assert!(matches!(err, Error::DecodeUtf8(_)));
    }
}
