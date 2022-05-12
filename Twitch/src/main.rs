use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::TwitchIRCClient;
use twitch_irc::{ClientConfig, SecureTCPTransport};
use twitch_irc::message::ServerMessage;
use futures::{join};
use std::result::Result as StdResult;
use warp::{Filter, Rejection, Reply};
use amiquip::{Connection, ExchangeDeclareOptions, ExchangeType, Publish};
use tokio::{task};

type WebResult<T> = StdResult<T, Rejection>;

// #1 Debug sends (We shouldn't recieve our own messages, everybody should recieve the messages we send)

#[tokio::main]
pub async fn main() {
    // default configuration is to join chat as anonymous.
    let config = ClientConfig::default();
    let (mut incoming_messages, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);


    let local = task::LocalSet::new();
    // first thing you should do: start consuming incoming messages,
    // otherwise they will back up.
    local.spawn_local(async move {
        println!("Starting Twitch Listen");
        let mut connection = Connection::insecure_open("amqp://guest:guest@localhost:5672").unwrap();
        let channel = connection.open_channel(None).unwrap();
        let exchange = channel.exchange_declare(
            ExchangeType::Topic,
            "chat",
            ExchangeDeclareOptions{
                durable: true,
                ..ExchangeDeclareOptions::default()
            },
        ).unwrap();

        let routing_key = "twitch".to_string();

        while let Some(message) = incoming_messages.recv().await {
            match message {
            ServerMessage::Privmsg(msg) => {
                let mut text = msg.message_text;
                for emote in &msg.emotes {
                    let url = format!("<img href='https://static-cdn.jtvnw.net/emoticons/v1/{}/3.0'>", emote.id);
                    text = text.replace(&emote.code, &url);
                }
                //for badge in &msg.badges {
                //    println!("name:{} version:{}", badge.name, badge.version);
                //}
                println!("(#{}) {}: {}", msg.channel_login, msg.sender.name, text);
                exchange.publish(Publish::new(format!("(#{}) {}: {}", msg.channel_login, msg.sender.name, text).as_bytes(), routing_key.clone())).unwrap();
            },
            //ServerMessage::Whisper(msg) => {
            //    println!("(w) {}: {}", msg.sender.name, msg.message_text);
            //},
            _ => {}
        }
        }
    });
    // - Just the freeware version -
    // Chat Inputs (Twitch, Youtube, Trovo, Facebook, Discord, etc)
    // Chat Outputs (^ Same)
    // Chat Window (Displayed on Stream, needs to be highly customizable)
    // Moderation Window (Not displayed on stream, allows for per-chat moderation commands easily)
    // Chat Bot system (Needs to be configurable)

    // Alert Inputs (Twitch, Youtube, Trovo, Facebook, StreamLabs, StreamElements, Paypal, Discord, etc)
    // Alert Window (Displayed on Stream, needs to be highly customizable)
    // Tops/Recents Display (Displayed on Stream, needs to be configurable)
    // Goal Display (Displayed on Stream, needs to be configurable)
    // Alert DB Layer (Keep track of alert history, allows us to track "tops" and "recents")
    // Moderation Window (Displays alert history, Displays "tops" and "recents", configure goals)

    // - Paid Version -
    // UI Around configuration of _everything_ that is configurable
    //   Chat connections, Bot connections, Chat Window, Alert Window, etc
    // Automating the process of starting up servers/running programs, configuring them

    // Badge Notes (Phase 2 - Work on later)
    //   Request data from "https://badges.twitch.tv/v1/badges/global/display" to get all global badges
    //   For each channel we join, query for that channel's custom badges (Need to get channel ID) https://badges.twitch.tv/v1/badges/channels/<ChannelID>/display
    //     If we allow for multiple simultaneous channel joins channel custom badges need to be stored _with_ the channel name associated as they are not unique

    // join a channel
    client.join("exlted".to_owned());
    
    // keep the tokio executor alive.
    // If you return instead of waiting the background task will exit.
    local.await;


    let health_route = warp::path!("health").and_then(health_handler);
    let routes = health_route;

    println!("Started server at localhost:8000");
    let _ = join!(
        warp::serve(routes).run(([0, 0, 0, 0], 8000))
        //rmq_listen(pool.clone())
    );
}

async fn health_handler() -> WebResult<impl Reply> {
    Ok("OK")
}
