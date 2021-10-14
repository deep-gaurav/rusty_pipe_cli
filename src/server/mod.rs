use std::{convert::Infallible, sync::Arc};

use crate::yt_downloader::YTDownloader;

use self::schema::{PlayerMessage, QueryRoot, Storage, SubscriptionRoot, ToPlayerMessages};
use async_graphql::{
    http::{playground_source, GraphQLPlaygroundConfig},
    EmptyMutation, Schema,
};
use async_std::prelude::*;
use async_std::sync::Mutex;
use futures::{
    channel::mpsc::{Receiver, Sender},
    SinkExt, StreamExt,
};
use surf::{http::mime, Body, StatusCode};
use tide::Response;

pub mod schema;
pub mod search;
pub mod stream;

pub async fn run_server(
    mut msg_receiver: Receiver<PlayerMessage>,
    msg_sender: Sender<ToPlayerMessages>,
    downloader:YTDownloader,
    port: u16,
) {
    let storage = Storage {
        to_player_message: Arc::new(Mutex::new(msg_sender)),
        from_player_message: Arc::new(Mutex::new(None)),
    };
    let sc = storage.clone();

    let receiver_task = async {
        while let Some(msg) = msg_receiver.next().await {
            if let Some(sen) = &mut *sc.from_player_message.lock().await {
                let out = sen.send(msg).await;
                match out {
                    Ok(_) => {}
                    Err(err) => log::warn!("Cant send {:#?}", err),
                }
            }
        }
    };

    let schema = Schema::build(QueryRoot {
        downloader
    }, EmptyMutation, SubscriptionRoot {})
        .data(storage)
        .finish();

    let mut app = tide::new();

    app.at("/graphql")
        .post(async_graphql_tide::endpoint(schema.clone()))
        .get(async_graphql_tide::Subscription::new(schema));
    app.at("/").get(|_| async move {
        let mut resp = Response::new(StatusCode::Ok);
        resp.set_body(Body::from_string(playground_source(
            GraphQLPlaygroundConfig::new("/graphql").subscription_endpoint("/graphql"),
        )));
        resp.set_content_type(mime::HTML);
        Ok(resp)
    });

    let tide_fut = app.listen(format!("0.0.0.0:{}", port));
    println!("Server running on http://localhost:{}", port);

    futures::join!(receiver_task, tide_fut);
}
