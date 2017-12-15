extern crate caper;
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
use message::{EchoRequest, EchoResponse};
use caper::service::{EncapsulatedMethod, MethodError, NewEncapService, NewEncapsulatedMethod};
use caper::dispatcher::{Registrant, ServiceRegistry};
use caper::codec::{MethodCodec, ProtobufCodec};
use caper::channel::{Channel, ChannelBuilder, ChannelOption};
use caper::stub::{RpcWrapper, StubCallFuture};
use caper::server::{Server, ServerOption};
use caper::protocol::Protocol;
use protobuf::Message;

mod message;

pub trait EchoService {
    type EchoFuture: Future<Item = EchoResponse, Error = MethodError> + 'static;
    type RevEchoFuture: Future<Item = EchoResponse, Error = MethodError> + 'static;

    fn echo(&self, msg: EchoRequest) -> Self::EchoFuture;

    fn rev_echo(&self, msg: EchoRequest) -> Self::RevEchoFuture;
}

#[allow(non_camel_case_types)]
#[derive(Clone)]
struct EchoEchoWrapper__<S: Clone>(S);

impl<S: EchoService + Clone> Service for EchoEchoWrapper__<S> {
    type Request = EchoRequest;
    type Response = EchoResponse;
    type Error = MethodError;
    type Future = <S as EchoService>::EchoFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        self.0.echo(req)
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone)]
struct EchoRevEchoWrapper__<S: Clone>(S);

impl<S: EchoService + Clone> Service for EchoRevEchoWrapper__<S> {
    type Request = EchoRequest;
    type Response = EchoResponse;
    type Error = MethodError;
    type Future = <S as EchoService>::RevEchoFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        self.0.rev_echo(req)
    }
}

pub struct EchoRegistrant<S> {
    provider: S,
}

impl<S> EchoRegistrant<S> {
    pub fn new(provider: S) -> Self {
        EchoRegistrant { provider }
    }
}

impl<S> Registrant for EchoRegistrant<S>
where
    S: EchoService + Clone + Send + Sync + 'static,
{
    fn methods(&self) -> Vec<(String, NewEncapService)> {
        let mut entries = vec![];
        let provider = &self.provider;

        let wrap = EchoEchoWrapper__(provider.clone());
        let method = EncapsulatedMethod::new(ProtobufCodec::new(), wrap);
        entries.push((
            "echo".to_string(),
            Box::new(NewEncapsulatedMethod::new(method)) as NewEncapService,
        ));

        let wrap = EchoRevEchoWrapper__(provider.clone());
        let method = EncapsulatedMethod::new(ProtobufCodec::new(), wrap);
        entries.push((
            "rev_echo".to_string(),
            Box::new(NewEncapsulatedMethod::new(method)) as NewEncapService,
        ));

        entries
    }
}

#[derive(Clone)]
pub struct EchoStub<'a> {
    echo_wrapper: RpcWrapper<'a, ProtobufCodec<EchoResponse, EchoRequest>>,
    rev_echo_wrapper: RpcWrapper<'a, ProtobufCodec<EchoResponse, EchoRequest>>,
}

impl<'a> EchoStub<'a> {
    pub fn new(channel: &'a Channel) -> Self {
        EchoStub {
            echo_wrapper: RpcWrapper::new(ProtobufCodec::new(), channel),
            rev_echo_wrapper: RpcWrapper::new(ProtobufCodec::new(), channel),
        }
    }

    pub fn echo(&'a self, msg: EchoRequest) -> StubCallFuture<'a, EchoResponse> {
        self.echo_wrapper
            .call((msg, "Echo".to_string(), "echo".to_string()))
    }

    pub fn rev_echo(&'a self, msg: EchoRequest) -> StubCallFuture<'a, EchoResponse> {
        self.rev_echo_wrapper
            .call((msg, "Echo".to_string(), "rev_echo".to_string()))
    }
}


// user visible from here

#[derive(Clone)]
struct Echo;

impl EchoService for Echo {
    type EchoFuture = Box<Future<Item = EchoResponse, Error = MethodError>>;

    type RevEchoFuture = Box<Future<Item = EchoResponse, Error = MethodError>>;

    fn echo(&self, msg: EchoRequest) -> Self::EchoFuture {
        let string = msg.msg;
        let mut response = EchoResponse::new();
        response.msg = string;
        let future = Ok(response).into_future();

        Box::new(future)
    }

    fn rev_echo(&self, msg: EchoRequest) -> Self::RevEchoFuture {
        let string = msg.msg;
        let rev: String = string.chars().rev().collect();
        let mut response = EchoResponse::new();
        response.msg = rev;
        let future = Ok(response).into_future();

        Box::new(future)
    }
}


fn main() {
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
        let server = Server::new(addr, server_option);
        server.start();
    });

    thread::sleep_ms(100);

    let (channel, backend) = core.run(ChannelBuilder::single_server(addr, handle, channel_option))
        .unwrap();
    core.execute(backend).unwrap();

    let echo = EchoStub::new(&channel);

    for i in 0..5 {
        let mut request = EchoRequest::new();
        request.set_msg(format!("hello from the other side, time {}", i));

        let (response, _) = echo.echo(request.clone()).wait().unwrap();
        println!("Client received: {}", response.get_msg());
        let (response, _) = echo.rev_echo(request).wait().unwrap();
        println!("Client received: {}", response.get_msg());
    }
}