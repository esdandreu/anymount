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

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let value = std::str::from_utf8(bytes)
            .map_err(|error| format!("control message was not valid UTF-8: {error}"))?;

        match value {
            "ping" => Ok(Self::Ping),
            "ready" => Ok(Self::Ready),
            "shutdown" => Ok(Self::Shutdown),
            "ack" => Ok(Self::Ack),
            _ => value
                .strip_prefix("error:")
                .map(|message| Self::Error(message.to_owned()))
                .ok_or_else(|| format!("unknown control message: {value}")),
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

    #[test]
    fn control_message_round_trips() {
        let encoded = ControlMessage::Ping.encode();
        let decoded = ControlMessage::decode(&encoded).expect("decode should work");
        assert_eq!(decoded, ControlMessage::Ping);
    }
}
