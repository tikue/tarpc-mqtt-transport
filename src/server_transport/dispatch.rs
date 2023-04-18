
use std::{collections::HashMap, fmt::{Debug}, ops::Deref, time::Duration};
use tarpc::{ClientMessage, Request, Response, server::BaseChannel, transport::channel::UnboundedChannel};
use paho_mqtt::{AsyncReceiver, Message, PropertyCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc::{UnboundedSender, UnboundedReceiver, error::SendError, unbounded_channel};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct ClientUid(String);
impl Deref for ClientUid {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Topic(String);
impl Deref for Topic {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct CorrelationData(Vec<u8>);
impl Deref for CorrelationData {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

fn extract_client_message<Req>(msg: &Message) -> Result<ClientMessage<Req>, crate::Error>
where Req: DeserializeOwned
{
    let m = serde_json::from_slice(msg.payload())?;
    Ok(m)
}

fn extract_topic(msg: &Message) -> Result<Topic, crate::Error> {
    let t = msg.properties()
        .get_string(PropertyCode::ResponseTopic)
        .ok_or(paho_mqtt::Error::General("Response topic property not found"))?;
    Ok(Topic(t))
}

fn extract_correlation_data(msg: &Message) -> Result<CorrelationData, crate::Error> {
    let cd = msg.properties()
        .get_binary(PropertyCode::CorrelationData)
        .ok_or(paho_mqtt::Error::General("CorrelationData property not found"))?;
    Ok(CorrelationData(cd))
}

fn extract_client_uid(topic: &Topic) -> Result<ClientUid, crate::Error> {
    if !topic.starts_with("/client/") {
        Err(paho_mqtt::Error::General("topic needs to start with /client/"))?;
    }
    Ok(ClientUid(topic.split("/").skip(2).next().unwrap().into()))
}

/// - Creates server channels based on client UID parsed from message topics.
/// - Forwards client messages to the corresponding channel.
/// - (TODO) Writes all responses to the mqtt transport.
/// - Cleans up dropped channels every 10 seconds.
pub struct Dispatch<Req, Res> {
    mqtt: AsyncReceiver<Option<Message>>,
    channels: HashMap<ClientUid, UnboundedSender<ClientMessage<Req>>>,
    incoming_tx: UnboundedSender<BaseChannel<Req, Res, UnboundedChannel<ClientMessage<Req>, Response<Res>>>>,
    responses_tx: UnboundedSender<Response<Res>>,
    responses: UnboundedReceiver<Response<Res>>,
}

pub type Incoming<Req, Res> =
    UnboundedReceiver<BaseChannel<Req, Res, UnboundedChannel<ClientMessage<Req>, Response<Res>>>>;

impl<Req, Res> Dispatch<Req, Res>
where Req: DeserializeOwned + Send + 'static,
      Res: Serialize + Send + 'static,
{
    pub fn spawn(mqtt: AsyncReceiver<Option<Message>>) -> Incoming<Req, Res> {
        let (incoming_tx, incoming) = unbounded_channel();
        let (responses_tx, responses) = unbounded_channel();
        let mut dispatch = Dispatch {
            mqtt,
            channels: HashMap::new(),
            incoming_tx,
            responses_tx,
            responses,
        };
        tokio::spawn(async move {
            if let Err(e) = dispatch.run().await {
                log::error!("Server transport crashed: {}", e);
            }
        });
        incoming
    }

    pub async fn run(&mut self) -> Result<!, crate::Error> {
        let mut garbage_collection_interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                Some(_res) = self.responses.recv() => {
                    // TODO: write responses into mqtt transport
                }
                Ok(result) = self.mqtt.recv() => {
                    match result {
                        None => continue, // Mqtt disconnecting
                        Some(msg) => {
                            let m = extract_client_message(&msg)?;
                            if let ClientMessage::Request(Request { context: _, ..  }) = m {
                                let _correlation = extract_correlation_data(&msg)?;
                                // set the correlation data on the context
                            }
                            let topic = extract_topic(&msg)?;
                            let client_uid = extract_client_uid(&topic)?;
                            if let Some(server_channel) = self.get_or_create_channel(&client_uid).await {
                                let _ = server_channel.send(m);
                            }
                        }
                    }
                }
                _ = garbage_collection_interval.tick() => {
                    self.remove_closed_channels();
                }

            }
        }
    }

    /// Returns None if new connections are no longer being accepted.
    async fn get_or_create_channel(&mut self, client_uid: &ClientUid) -> Option<&mut UnboundedSender<ClientMessage<Req>>> {
        let mut new_channel = None;
        self.channels.entry(client_uid.clone()).or_insert_with(|| {
            // new client; create a BaseChannel
            let (requests_tx, requests) = unbounded_channel();
            let channel = UnboundedChannel::join(self.responses_tx.clone(), requests);
            new_channel = Some(BaseChannel::with_defaults(channel));
            requests_tx
        });
        let Some(channel) = new_channel else { return self.channels.get_mut(client_uid) };
        match self.incoming_tx.send(channel) {
            Ok(()) => self.channels.get_mut(client_uid),
            Err(SendError(_)) => {
                self.channels.remove(client_uid);
                None
            }
        }
    }

    fn remove_closed_channels(&mut self) {
        for _ in self.channels.drain_filter(|_, channel| channel.is_closed()) {
        }
    }
}
