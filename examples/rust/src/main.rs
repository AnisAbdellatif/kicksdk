//! Prints a Kick channel's chat:  cargo run -- xqc
//!
//! kicksdk builds the requests and speaks the protocol; ureq sends the
//! requests and tungstenite holds the socket.

use kicksdk::events::KickEvent;
use kicksdk::realtime::{self, Action, Backoff, Session, SessionOptions};
use kicksdk::{Endpoints, Method, Request, website};
use std::time::Instant;
use tungstenite::client::IntoClientRequest;

fn send(http: &ureq::Agent, r: &Request) -> (u16, String) {
    let mut res = match r.method {
        Method::Get => r.headers.iter().fold(http.get(&r.url), |q, (k, v)| q.header(k, v)).call(),
        _ => r.headers.iter().fold(http.post(&r.url), |q, (k, v)| q.header(k, v)).send(r.body.as_deref().unwrap_or("")),
    }
    .expect("request failed");
    (res.status().as_u16(), res.body_mut().read_to_string().unwrap_or_default())
}

fn main() {
    let slug = std::env::args().nth(1).unwrap_or_else(|| "xqc".into());
    let e = Endpoints::default();
    let http: ureq::Agent =
        ureq::Agent::config_builder().http_status_as_error(false).user_agent("Mozilla/5.0").build().into();

    let (s, b) = send(&http, &website::channel_request(&e, &slug));
    let channel = website::parse_channel(s, &b).expect("no such channel");
    let mut session = Session::new(SessionOptions::new(realtime::chat_channels(channel.chatroom_id, Some(channel.id))));
    let mut backoff = Backoff::default();
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;

    loop {
        let client_id = uuid::Uuid::new_v4().to_string();
        let (s, b) = send(&http, &realtime::chat_connection_request(&e, channel.id, &client_id));
        let url = realtime::parse_connection(s, &b).unwrap()[0].centrifugo_url().unwrap().to_owned();
        let (s, b) = send(&http, &realtime::token_request(&e, &client_id));
        let token = realtime::parse_token(s, &b).unwrap();

        let mut req = url.into_client_request().unwrap();
        req.headers_mut().insert("Origin", realtime::REQUIRED_ORIGIN.parse().unwrap());
        let (mut ws, _) = tungstenite::connect(req).expect("connect failed");

        let mut actions = session.open(&token, now());
        'socket: loop {
            for action in std::mem::take(&mut actions) {
                match action {
                    Action::Send { text } => ws.send(text.into()).unwrap(),
                    Action::Close { reason } => {
                        eprintln!("[{reason}]");
                        break 'socket;
                    }
                    Action::RefreshToken => {
                        let (s, b) = send(&http, &realtime::token_request(&e, &client_id));
                        actions.extend(session.refresh(&realtime::parse_token(s, &b).unwrap(), now()));
                    }
                    Action::Subscribed { .. } => backoff.reset(),
                    Action::Event { event: KickEvent::ChatMessage(m), .. } => {
                        println!("{}: {}", m.sender.username, m.content)
                    }
                    _ => {}
                }
            }
            // Kick pings every 25 s, so a frame always comes to wake the loop.
            match ws.read() {
                Ok(tungstenite::Message::Text(t)) => actions.extend(session.receive(&t, now())),
                Ok(_) => {}
                Err(_) => break,
            }
            actions.extend(session.tick(now()));
        }
        session.closed();
        std::thread::sleep(std::time::Duration::from_millis(backoff.next_delay()));
    }
}
