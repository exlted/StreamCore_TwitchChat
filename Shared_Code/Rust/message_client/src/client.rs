use amqprs::{
    channel::{
        Channel as AMQPRS_Channel,
        BasicPublishArguments, BasicConsumeArguments, QueueBindArguments, QueueDeclareArguments, ExchangeDeclareArguments
    },
    connection::{
        Connection,
        OpenConnectionArguments
    },
    error::Error,
    callbacks::{ChannelCallback, ConnectionCallback},
    Close, Ack, Nack, Return, Cancel, CloseChannel, BasicProperties,
    consumer::AsyncConsumer
};
use std::{
    mem,
    ptr,
    sync::Arc
};
use tokio::{
    sync::Mutex,
    time
};
use async_trait::async_trait;


pub type Result<T> = std::result::Result<T, Error>;

pub struct Channel {
    connection: Arc<Mutex<Client>>,
    queue_name: String,
    pub(crate) channel: Option<AMQPRS_Channel>,
    pub(crate) open: bool,
    pub(crate) flow_blocked: bool,
    pub exchange_name: String,
    pub routing_key: String,
}

impl Channel {

    pub(crate) async fn new(connection: Arc<Mutex<Client>>, exchange_name: String, routing_key: String) -> Arc<Mutex<Channel>>
    {
        let channel = Arc::new(Mutex::new(Channel {
            connection,
            queue_name: "".to_string(),
            channel: None,
            open: false,
            flow_blocked: false,
            exchange_name,
            routing_key,
        }));

        CHANNELS.lock().await.push(channel.clone());

        return channel
    }

    pub(crate) async fn open_channel<F>(&mut self, channel_callback: F) -> Option<()>
        where F: ChannelCallback + Send + 'static,
    {
         self.channel = Some(self.connection.lock().await.connection.as_ref()?.open_channel(None).await.unwrap());
         self.channel.as_ref()?
             .register_callback(channel_callback)
             .await
             .unwrap();

         let mut exchange_opts = ExchangeDeclareArguments::new(&self.exchange_name.clone(), "topic");
         exchange_opts.durable = true;

         self.channel.as_ref()?
             .exchange_declare(exchange_opts).await.unwrap();

         let (queue_name, _, _) = self.channel.as_ref()?
             .queue_declare(QueueDeclareArguments::default())
             .await
             .unwrap()
             .unwrap();

         self.queue_name = queue_name;

         self.channel.as_ref()?
             .queue_bind(QueueBindArguments::new(
                 &*self.queue_name.clone(),
                 &*self.exchange_name.clone(),
                 &*self.routing_key.clone()
             ))
             .await
             .unwrap();

        Some(())
    }

    pub async fn close(&mut self) -> Option<()> {
        if !self.is_connection_good().await {
            return None
        }

        mem::replace(&mut self.channel, None)?.close().await.unwrap();

        Some(())
    }

    async fn is_connection_good(&self) -> bool {
        if !self.open || self.flow_blocked || !self.connection.lock().await.is_connection_good() {
            return false;
        }
        return true;
    }

    pub async fn publish_message(&self, message: String) -> Option<()> {
        if !self.is_connection_good().await {
            return None
        }

        let content = message.into_bytes();

        let args = BasicPublishArguments::new(&*self.exchange_name.clone(), &*self.routing_key.clone());

        self.channel.as_ref()?
            .basic_publish(BasicProperties::default(), content, args)
            .await
            .unwrap();

        Some(())
    }

    pub async fn attach_consumer<F>(&mut self, name: String, consumer: F) -> Option<()>
    where
        F: AsyncConsumer + Send + 'static
    {
        if !self.is_connection_good().await {
            return None
        }

        let args = BasicConsumeArguments::new(
            &*self.queue_name.clone(),
            &*name
        );

        self.channel.as_ref()?
            .basic_consume(consumer, args)
            .await
            .unwrap();

        Some(())
    }
}

pub struct Client {
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub(crate) connection: Option<Connection>,
    pub(crate) channels: Vec<Arc<Mutex<Channel>>>,
    pub(crate) open: bool,
    pub(crate) blocked: bool
}

static CLIENTS: Mutex<Vec<Arc<Mutex<Client>>>> = Mutex::const_new(Vec::new());
static CHANNELS: Mutex<Vec<Arc<Mutex<Channel>>>> = Mutex::const_new(Vec::new());

pub struct BasicConnectionCallback;

#[async_trait]
impl ConnectionCallback for BasicConnectionCallback {

    async fn close(&mut self, connection: &Connection, _close: Close) -> Result<()> {

        let clients = CLIENTS.lock().await;
        for client_mutex in clients.iter() {
            let mut client = client_mutex.lock().await;
            if client.open && client.connection.is_some() {
                let client_connection = client.connection.as_ref().unwrap();
                if ptr::eq(client_connection, connection) {
                    client.close_client().await;
                }
            }
        }

        Ok(())
    }

    async fn blocked(&mut self, connection: &Connection, _reason: String) {
        let clients = CLIENTS.lock().await;
        for client_mutex in clients.iter() {
            let mut client = client_mutex.lock().await;
            if client.open && client.connection.is_some() {
                let client_connection = client.connection.as_ref().unwrap();
                if ptr::eq(client_connection, connection) {
                    client.blocked = true;
                }
            }
        }
    }

    async fn unblocked(&mut self, connection: &Connection) {
        let clients = CLIENTS.lock().await;
        for client_mutex in clients.iter() {
            let mut client = client_mutex.lock().await;
            if client.open && client.connection.is_some() {
                let client_connection = client.connection.as_ref().unwrap();
                if ptr::eq(client_connection, connection) {
                    client.blocked = false;
                }
            }
        }
    }
}

pub struct BasicChannelCallback;

#[async_trait]
impl ChannelCallback for BasicChannelCallback {
    async fn close(&mut self, channel: &AMQPRS_Channel, _close: CloseChannel) -> Result<()> {
        let channels = CHANNELS.lock().await;
        for channel_mutex in channels.iter() {
            let mut client_object = channel_mutex.lock().await;
            if client_object.open && client_object.connection.lock().await.connection.is_some() {
                let client_channel = client_object.channel.as_ref().unwrap();
                if ptr::eq(client_channel, channel) {
                    client_object.close().await;
                }
            }
        }
        Ok(())
    }
    async fn cancel(&mut self, channel: &AMQPRS_Channel, _cancel: Cancel) -> Result<()> {
        let channels = CHANNELS.lock().await;
        for channel_mutex in channels.iter() {
            let mut chanel_object = channel_mutex.lock().await;
            if chanel_object.open && chanel_object.connection.lock().await.connection.is_some() {
                let client_channel = chanel_object.channel.as_ref().unwrap();
                if ptr::eq(client_channel, channel) {
                    chanel_object.close().await;
                }
            }
        }
        Ok(())
    }
    async fn flow(&mut self, channel: &AMQPRS_Channel, active: bool) -> Result<bool> {
        let channels = CHANNELS.lock().await;
        for channel_mutex in channels.iter() {
            let mut channel_object = channel_mutex.lock().await;
            if channel_object.open && channel_object.connection.lock().await.connection.is_some() {
                let client_channel = channel_object.channel.as_ref().unwrap();
                if ptr::eq(client_channel, channel) {
                    channel_object.flow_blocked = active;
                }
            }
        }
        Ok(active)
    }
    async fn publish_ack(&mut self, channel: &AMQPRS_Channel, ack: Ack) {
        println!(
            "Info: handle publish ack delivery_tag={} on channel {}",
            ack.delivery_tag(),
            channel
        );
    }
    async fn publish_nack(&mut self, channel: &AMQPRS_Channel, nack: Nack) {
        println!(
            "Warning: handle publish nack delivery_tag={} on channel {}",
            nack.delivery_tag(),
            channel
        );
    }
    async fn publish_return(
        &mut self,
        channel: &AMQPRS_Channel,
        ret: Return,
        _basic_properties: BasicProperties,
        content: Vec<u8>,
    ) {
        println!(
            "Warning: handle publish return {} on channel {}, content size: {}",
            ret,
            channel,
            content.len()
        );
    }
}

impl Client {
    pub async fn new(host: String, port: String, username: String, password: String) -> Arc<Mutex<Client>> {
        let client = Arc::new(Mutex::new(Client {
            host,
            port,
            username,
            password,
            connection: None,
            channels: vec!(),
            open: false,
            blocked: false
        }));

        CLIENTS.lock().await.push(client.clone());

        return client;
    }

    pub fn is_connection_good(&self) -> bool
    {
        if !self.open || self.blocked {
            return false;
        }
        return true;
    }

    pub async fn open_client<F>(&mut self, connection_callback: F) -> Option<()>
    where
        F: ConnectionCallback + Send + 'static,
    {
        let mut attempt_cnt = 5;
        loop {
            let connection_attempt = Connection::open(&OpenConnectionArguments::new(
                &*self.host.clone(),
                self.port.clone().parse().unwrap(),
                &*self.username.clone(),
                &*self.password.clone()
            )).await;
            if connection_attempt.is_ok() {
                self.connection = Some(connection_attempt.unwrap());
                break;
            } else {
                attempt_cnt = attempt_cnt - 1;
                if attempt_cnt <= 0 {
                    return None;
                }
                let mut interval = time::interval(time::Duration::from_secs(2));
                interval.tick().await;
            }
        }

        self.connection.as_ref()?
            .register_callback(connection_callback)
            .await
            .unwrap();

        self.open = true;

        Some(())
    }

    pub async fn close_client(&mut self) -> Option<()> {
        if !self.open {
            return None
        }

        for channel in &mut self.channels {
            channel.lock().await.close().await;
        }
        self.channels.clear();

        mem::replace(&mut self.connection, None)?.close().await.unwrap();

        self.open = false;
        Some(())
    }

    pub async fn open_channel<F>(&mut self, connection: Arc<Mutex<Client>>, exchange_name: String, routing_key: String, channel_callback: F)
        ->Option<Arc<Mutex<Channel>>>
        where F: ChannelCallback + Send + Sized + 'static
    {
        let rv = Channel::new(connection, exchange_name, routing_key).await;

        rv.lock().await.open_channel(channel_callback).await;

        self.channels.push(rv.clone());

        Some(rv)
    }
}