use std::fmt::{Debug};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Poll;
use tarpc::{ClientMessage, Response};
use futures::{prelude::*};
use paho_mqtt::{AsyncReceiver, DeliveryToken, Message, MessageBuilder, Properties, PropertyCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use log::warn;

#[pin_project]
pub struct ServerTransport<Req> {
    #[pin]
    client: paho_mqtt::AsyncClient,
    #[pin]
    stream: AsyncReceiver<Option<Message>>,
    #[pin]
    delivery_token: Option<DeliveryToken>,

    request_topic: String,
    phantom: PhantomData<Req>
}

#[derive(Debug, Clone, Default)]
pub struct ServerContext {
    pub response_topic: String,
    pub correlation: Vec<u8>
}


impl<Req> ServerTransport<Req> {
    pub fn new<T: Into<String>>(mut client: paho_mqtt::AsyncClient, request_topic: T) -> ServerTransport<Req> {
        let request_topic = request_topic.into();
        let stream = client.get_stream(25);

        let rt = request_topic.clone();
        client.set_connected_callback(move |cli| {
            tokio::spawn(cli.subscribe(rt.clone(), 1));
        });

        client.subscribe(request_topic.clone(), 1);

        ServerTransport { client, request_topic: request_topic, stream, delivery_token: None, phantom: PhantomData::default() }
    }
}

impl<Req, Res> Sink<(ServerContext, Response<Res>)> for ServerTransport<Req> where Res: Debug + Serialize {
    type Error = crate::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        let pinref = &mut self.delivery_token;

        match pinref {
            None => Poll::Ready(Ok(())),
            Some(f) => Pin::new(f).poll(cx).map_err(Into::into)
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: (ServerContext, Response<Res>)) -> Result<(), Self::Error> {
        let (context, response) = item;
        let data = serde_json::to_vec(&response)?;

        let mut props = Properties::new();
        props.push_binary(PropertyCode::CorrelationData, context.correlation)?;

        let msg = MessageBuilder::new().payload(data).topic(&context.response_topic).qos(1).properties(props).finalize();

        let delivery_token = (&mut self.client).publish(msg);

        self.delivery_token = Some(delivery_token);

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        let pinref = &mut self.delivery_token;

        match pinref {
            None => Poll::Ready(Ok(())),
            Some(f) => {
                match Pin::new(f).poll(cx) {
                    Poll::Ready(r) => {
                        self.delivery_token.take();
                        Poll::Ready(r).map_err(Into::into)
                    },
                    Poll::Pending => Poll::Pending
                }
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }
}

impl<Req> Stream for ServerTransport<Req> where Req: DeserializeOwned + Debug {
    type Item = Result<(ServerContext, ClientMessage<Req>), crate::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            let e : Result<!, crate::Error> = match this.stream.as_mut().poll_next(cx) {
                Poll::Ready(None) => break Poll::Ready(None),
                Poll::Ready(Some(None)) => continue, // Mqtt Disconnecting
                Poll::Ready(Some(Some(msg))) => try {
                    let m: ClientMessage<Req> = serde_json::from_slice(msg.payload())?;
                    let topic = msg.properties().get_string(PropertyCode::ResponseTopic).ok_or(paho_mqtt::Error::General("Response topic property not found"))?;
                    let correlation = msg.properties().get_binary(PropertyCode::CorrelationData).ok_or(paho_mqtt::Error::General("CorrelationData property not found"))?;

                    let context = ServerContext { correlation, response_topic: topic };

                    break Poll::Ready(Some(Ok((context, m))))
                },
                Poll::Pending => break Poll::Pending
            };

            warn!("ServerTransport: Dropping malformed MQTT Message {:?}", e);
        }
    }
}
