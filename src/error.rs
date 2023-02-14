use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    Paho(paho_mqtt::Error),
    Serde(serde_json::Error)
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Paho(value) => Display::fmt(value, f),
            Error::Serde(value) => Display::fmt(value,f)
        }
    }
}

impl From<paho_mqtt::Error> for Error {
    fn from(value: paho_mqtt::Error) -> Self {
        Error::Paho(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::Serde(value)
    }
}
