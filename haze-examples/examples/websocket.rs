#![allow(clippy::unused_async)]

use dioxus::{
    fullstack::{WebSocketOptions, Websocket},
    prelude::*,
};

#[cfg(feature = "server")]
use {
    dioxus::fullstack::TypedWebsocket,
    haze::{Pack, Res},
};

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    haze::serve(
        async |resources| {
            resources.insert(Prefix(String::from("echo")));
            resources.insert(Repeat(2));
            Ok(())
        },
        || dioxus::server::router(App),
    );
}

#[cfg(feature = "server")]
#[derive(Clone)]
struct Prefix(String);

#[cfg(feature = "server")]
#[derive(Clone)]
struct Repeat(usize);

#[cfg(feature = "server")]
#[derive(Clone, Pack)]
struct Echo {
    prefix: Prefix,
    repeat: Repeat,
}

#[cfg(feature = "server")]
impl Echo {
    fn reply(&self, text: &str) -> String {
        format!("{}: {}", self.prefix.0, text.repeat(self.repeat.0))
    }
}

#[get("/api/echo", echo: Res<Echo>)]
async fn echo(options: WebSocketOptions) -> Result<Websocket<String, String>> {
    Ok(options.on_upgrade(
        move |mut socket: TypedWebsocket<String, String>| async move {
            while let Ok(text) = socket.recv().await {
                if socket.send(echo.reply(&text)).await.is_err() {
                    return;
                }
            }
        },
    ))
}

#[component]
fn App() -> Element {
    let mut replies = use_signal(Vec::<String>::new);
    let socket = haze::use_websocket(
        || echo(WebSocketOptions::new()),
        move |reply| replies.push(reply),
    );
    rsx! {
        p { "{socket.status():?}" }
        input {
            placeholder: "Type and press enter",
            onchange: move |event| async move {
                _ = socket.send(event.value()).await;
            },
        }
        for reply in replies.read().iter() {
            p { "{reply}" }
        }
    }
}
