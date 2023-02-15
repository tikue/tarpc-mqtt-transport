[![Crates.io][crates-badge]][crates-url]
[![MIT licensed][mit-badge]][mit-url]

[//]: # ([![Build status][gh-actions-badge]][gh-actions-url])
[//]: # ([![Discord chat][discord-badge]][discord-url])

[crates-badge]: https://img.shields.io/crates/v/tarpc-mqtt-transport.svg
[crates-url]: https://crates.io/crates/tarpc-mqtt-trapnsport
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: LICENSE
[gh-actions-badge]: https://github.com/axos88/tarpc-mqtt-transport/workflows/Continuous%20integration/badge.svg
[gh-actions-url]: https://github.com/axos88/tarpc-mqtt-transport/actions?query=workflow%3A%22Continuous+integration%22
[discord-badge]: https://img.shields.io/discord/647529123996237854.svg?logo=discord&style=flat-square
[discord-url]: https://discord.gg/gXwpdSt

### !! This crate only works with a modified version of tarpc until https://github.com/google/tarpc/issues/392 is merged

# tarpc

tarpc is an RPC framework for rust with a focus on ease of use. Defining a
service can be done in just a few lines of code, and most of the boilerplate of
writing a server is taken care of for you.

[Documentation](https://docs.rs/crate/tarpc/)

# tarpc-mqtt-transport

tarpc-mqtt-transport is a transport plugin for tarpc that allows rpc calls to 
be made over an MQTTv5 broker. The rpc server subscribes to a topic known to the 
clients intending to make calls


There are some differences to how other tarpc transport work that needs to be kept
in mind:
  - All clients publish their requests to the same topic, thus the server sees all requests
from all clients in an aggregated fashion, without the server being able to differentiate
between them. Since tarpc requires unique request ids, a client (malicious, broken or 
otherwise) could wreak havoc if they could guess or cause conflicts with the request ids
of other clients. Thus it is strongly recommended to use the sequencer in the mqtt transport
generating a random permutation of the u64 numbers instead of the default monotonic counter
implemented in tarpc.
  - Usually if a malicious or broken client spews out invalid messages, its connection is closed,
end of story. This is not a possibility for MQTT, since all clients share the same topic to publish
requests, and the server doesn't know which client sent which message. Thus such invalid messages
are simply ignored, and the error is logged.
  - The clients need to subscribe to a response topic that is unique to them. The server needs to be
instructed to send the response to their requests to that topic using the ResponseTopic property of the
MQTTv5 messages. The CorrelationData SHOULD be set to the request id, but the server treats it as an 
opaque value.
  - The requests and responses are currently set up to be serialized as json, but any data format supported
by serde could be supported in the future (such as messagepack or bincode)
  - Security-wise it is recommended to set up ACLs so that only the server SHOULD be able to subscribe to 
the request topic and the response topics advertised by the clients SHOULD be set up so that only that 
specific client can subsribe to it, and only the server SHOULD be able to publish to it. If the list of 
clients is known beforehand, the request topic MAY be set up so that only the clients can publish to it.


## Usage
Add to your `Cargo.toml` dependencies:

```toml
paho-mqtt = "0.12.0"
tokio = { version = "1", features = ["full"] }
tarpc = { git = "https://github.com/axos88/tarpc.git", branch = "request-context", features = ["full"] }
tarpc-mqtt-transport = { git = "https://github.com/axos88/tarpc-mqtt-transport.git" }
```

## Example

```rust 
#[tarpc::service]
pub trait PingService {
     async fn ping() -> u8;
}

#[derive(Clone)]
struct Service(Arc<RwLock<u8>>);

impl PingService for Service {
    async fn ping(self, _: Context) -> u8 {
        let mut g = self.0.write().unwrap();
         *g += 1;
        log::info!("Ping called: {:?}", *g );

        *g * 2
    }
}

async fn spawn(fut: impl Future<Output = ()> + Send + 'static) {
    tokio::spawn(fut);
}

const SERVER_TOPIC: &'static str = "requests";

pub async fn main() -> anyhow::Result<()> {
    let _server = tokio::spawn(async move {
        let mqtt_client = build_mqtt_client(/* Create a connected MQTT Async client */)).await;
        let transport = tarpc_mqtt_transport::ServerTransport::new(mqtt_client, SERVER_TOPIC);

        let service = Service(Arc::new(RwLock::new(0)));

        ContextualChannel::with_defaults(transport).execute(service.serve()).for_each(spawn).await;
    });

    let mqtt_client =   build_mqtt_client(/* Create another connected MQTT Async client */).await;
    let transport = tarpc_mqtt_transport::ClientTransport::new(mqtt_client, SERVER_TOPIC, "responses/my_clientid");

    let config = Config::with_sequencer(tarpc_mqtt_transport::MqttRequestSequencer::random());

    let client = PingServiceClient::new(config, transport).spawn();

    log::info!("{:?}", client.ping(tarpc::context::current()).await); // Ping called: 1; Ok(2)
    log::info!("{:?}", client.ping(tarpc::context::current()).await); // Ping called: 2; Ok(4)
    log::info!("{:?}", client.ping(tarpc::context::current()).await); // Ping called: 3; Ok(6)
    log::info!("{:?}", client.ping(tarpc::context::current()).await); // Ping called: 4; Ok(8)

    log::info!("Dropping server and client");

    Ok(())
}
```

