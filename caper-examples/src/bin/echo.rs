extern crate caper;
extern crate caper_examples;
extern crate env_logger;
extern crate futures;
extern crate protobuf;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;

use futures::{Future, IntoFuture};
use futures::future::Executor;
use std::thread;
use std::time::Duration;
use tokio_service::{NewService, Service};
use tokio_core::reactor::Core;
use caper::dispatcher::{Registrant, ServiceRegistry};
use caper::service::MethodError;
use caper::controller::Controller;
use caper::channel::{Channel, ChannelBuilder, ChannelOption};
use caper::stub::{RpcWrapper, StubCallFuture};
use caper::server::{Server, ServerOption};
use caper::protocol::Protocol;

use caper_examples::protos::echo::{EchoRequest, EchoResponse};
use caper_examples::protos::echo_caper::{EchoService, EchoStub, EchoRegistrant};

// user visible from here

#[derive(Clone)]
struct Echo;

impl EchoService for Echo {
    type EchoFuture = Box<Future<Item = (EchoResponse, Controller), Error = MethodError>>;

    type RevEchoFuture = Box<Future<Item = (EchoResponse, Controller), Error = MethodError>>;

    fn echo(&self, msg: (EchoRequest, Controller)) -> Self::EchoFuture {
        let (msg, controller) = msg;
        let string = msg.msg;
        let mut response = EchoResponse::new();
        response.msg = string;
        let future = Ok(response).into_future()
            .map(move |resp| (resp, controller));

        Box::new(future)
    }

    fn rev_echo(&self, msg: (EchoRequest, Controller)) -> Self::RevEchoFuture {
        let (msg, controller) = msg;
        let string = msg.msg;
        let rev: String = string.chars().rev().collect();
        let mut response = EchoResponse::new();
        response.msg = rev;
        let future = Ok(response).into_future()
            .map(move |resp| (resp, controller));

        Box::new(future)
    }
}

fn main() {
    env_logger::init().unwrap();

    let addr = "127.0.0.1:8989";
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let registrant = EchoRegistrant::new(Echo);
    let mut registry = ServiceRegistry::new();
    registry.register_service(&"Echo".to_string(), registrant);
    let server_option = ServerOption {
        protocols: vec![Protocol::Brpc],
    };
    let channel_option = ChannelOption::new();
    thread::spawn(move || {
        let server = Server::new(addr, server_option, registry);
        server.start();
    });

    thread::sleep_ms(100);

    let (channel, backend) = core.run(ChannelBuilder::single_server(
        addr,
        handle.clone(),
        channel_option,
    )).unwrap();

    handle.spawn(backend);

    let echo = EchoStub::new(&channel);

    for i in 0..5 {
        let mut request = EchoRequest::new();
        request.set_msg(format!("hello from the other side, time {}", i));

        let fut = echo.echo(request.clone())
            .map_err(move |e| println!("Request {} failed with {:?}", i, e))
            .and_then(|(msg, _)| {
                println!("Client received: {}", msg.get_msg());
                Ok(())
            });
        core.run(fut).unwrap();

        let fut = echo.rev_echo(request)
            .map_err(move |e| println!("Request {} failed with {:?}", i, e))
            .and_then(|(msg, _)| {
                println!("Client received: {}", msg.get_msg());
                Ok(())
            });
        core.run(fut).unwrap();
    }
}
