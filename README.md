RPC framework in Rust

[![Build Status](https://travis-ci.org/AprliRainkun/copra.svg?branch=master)](https://travis-ci.org/AprliRainkun/copra)
[![Crates.io](https://img.shields.io/crates/v/copra.svg)](https://crates.io/crates/copra)
[![Docs.rs](https://docs.rs/copra/badge.svg)](https://docs.rs/copra)

`copra` is an [RPC] framework aimed at ease of use and configuration.
It can generate most of the boilerplate code in server and client side.
You only need to implement the core logic of services.

[RPC]: https://en.wikipedia.org/wiki/Remote_procedure_call

## Installation

### Protocol compiler installation

`copra` uses [Protocol Buffers][protobuf] (a.k.a. protobuf) to exchange messages
and describe service signatures. The message and service descriptions are written
in `.proto` files, and `copra` depends on the protocol compiler to generate rust
code from these files.

Visit [this website] and download
`proto-3.*.*-your-arch.zip` (`copra` needs protocol version 3), extract the
`protoc` executable to a folder you like, then add `protoc` to your `PATH`.

[protobuf]: https://developers.google.com/protocol-buffers/
[this website]: https://github.com/google/protobuf/releases

### Cargo setup

Add this to your `Cargo.toml`:

```toml
[dependencies]
copra = "0.1"
futures = "0.1"
tokio-core = "0.1"

[build-dependencies]
protoc-rust-copra = "0.1"
```

## Quick start

Here is an example of implementing an echo RPC. First, create a file named
`echo.proto` and put it in the manifest directory (i.e. next to `Cargo.toml`).
Populate it with:

```protobuf
syntax = "proto3"

message EchoMessage {
    string msg = 1;
}

// Our echo service contains two method. One is sending back the original string
// directly, and the other is returning the string in reversed form.
service Echo {
    rpc echo(EchoMessage) returns (EchoMessage);
    rpc reverse_echo(EchoMessage) returns (EchoMessage);
}
```

Next, create a [`build.rs`][build-scripts] in the manifest directory, and add this to
it:

```rust
extern crate protoc_rust_copra;

fn main() {
    protoc_rust_copra::run(protoc_rust_copra::Args {
        out_dir: "src/protos",
        input: &["echo.proto"],
        includes: &[],
        rust_protobuf: true
    }).expect("Failed to compile proto files");
}
```

This will generate file `echo.rs` and `echo_copra.rs` in `src/protos`.

Then, add this to `main.rs`:

```rust
extern crate copra;
extern crate futures;
extern crate tokio_core;

use copra::{ChannelBuilder, Controller, MethodError, ServerBuilder, ServiceRegistry};
use futures::future::{self, Future, FutureResult};
use std::thread;
use tokio_core::reactor::Core;

use protos::echo::EchoMessage;
use protos::echo_copra::{EchoRegistrant, EchoService, EchoStub};

mod protos;

// Service provider must implement Clone
#[derive(Clone)]
struct Echo;

// EchoService is a trait for defining service logic
// It is generated by protoc-rust-copra
impl EchoService for Echo {
    type EchoFuture = FutureResult<(EchoMessage, Controller), MethodError>;

    type ReverseEchoFuture = FutureResult<(EchoMessage, Controller), MethodError>;

    fn echo(&self, (req, ctrl): (EchoMessage, Controller)) -> Self::EchoFuture {
        let mut response = EchoMessage::new();
        response.set_msg(req.msg);
        future::ok((response, ctrl))
    }

    fn reverse_echo(
        &self,
        (req, ctrl): (EchoMessage, Controller)
    ) -> Self::ReverseEchoFuture {
        let rev: String = req.msg.chars().rev().collect();
        let mut response = EchoMessage::new();
        response.set_msg(rev);
        future::ok((response, ctrl))
    }
}

fn main() {
    let addr = "127.0.0.1:8989";

    // server side
    thread::spawn(move || {
        // register the service provider, so that it can be accessed
        let registrant = EchoRegistrant::new(Echo);
        let mut registry = ServiceRegistry::new();
        registry.register_service(registrant);

        let server = ServerBuilder::new(addr, registry).build().unwrap();
        server.start();
    });

    // client side
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let channel = core.run(ChannelBuilder::single_server(addr, handle).build())
        .unwrap();
    let stub = EchoStub::new(&channel);

    let mut request = EchoMessage::new();
    request.set_msg("Hello world".to_string());

    let (response, _info) = core.run(stub.echo(request.clone())).unwrap();
    println!("{}", response.msg);

    let (response, _info) = core.run(stub.reverse_echo(request)).unwrap();
    println!("{}", response.msg);
}
```

Finally, build and run this example by executing:

```bash
$ cargo build
$ cargo run
```

More examples can be found in `copra-examples`.

[build-scripts]: https://doc.rust-lang.org/cargo/reference/build-scripts.html

## Project structure

* `copra`: main crate
* `copra-compile`: generate protobuf runtime code used in the main `copra` crate
* `copra-examples`: runnable examples
* `protoc-rust-copra`: codegen for copra

## Note

This project is still in the early development stage. It basically works, but 
you should use it with caution. Any form of contribution is appreciated.

## License

copra is free and open source software distributed under the terms of both the
[MIT License] and the [Apache License 2.0].

## Change log
### `copra`

* v0.1.1: Add homepage and documentation in cargo manifest files.

### `protoc-rust-copra`

* v0.1.1: Add homepage and documentation in cargo manifest files.

[MIT License]: LICENSE-MIT
[Apache License 2.0]: LICENSE-APACHE

## Acknowledgement

This project is inspired by the [brpc] framework developped by Baidu Inc. copra
has similar interface and message protocol to brpc framework.

[brpc]: https://github.com/brpc/brpc
