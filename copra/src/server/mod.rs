//! Accept connections and provide services
//!
//! # Examples
//!
//! ```no_run
//! # extern crate copra;
//! # extern crate tokio_core;
//! # use std::error::Error;
//! use copra::{ServiceRegistry, ServerBuilder};
//! # fn main() {
//! #     try_main().unwrap();
//! # }
//! # fn try_main() -> Result<(), Box<Error>> {
//!
//! let mut registry = ServiceRegistry::new();
//!
//! // add some service to the registry
//!
//! let server = ServerBuilder::new("127.0.0.1:8000", registry).build()?;
//! server.start();
//! # Ok(())
//! # }
//! ```

use bytes::Bytes;
use tokio_core::reactor::Remote;
use tokio_proto::TcpServer;
use tokio_proto::multiplex::Multiplex;
use tokio_service::{NewService, Service};
use tokio_timer::Timer;
use std::error::Error;
use std::fmt;
use std::io;
use std::net::AddrParseError;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use futures::{Future, IntoFuture, Stream};
use futures::future::Executor;

use controller::Controller;
use protocol::Protocol;
use dispatcher::ServiceRegistry;
use service::MethodError;
use message::RpcResponseMeta;
use message::{RequestPackage, ResponsePackage};
use monitor::ThroughputMaintainer;

use self::protocol::MetaServerProtocol;

mod connection;
mod protocol;

type Second = u64;

type MetaServiceFuture = Box<Future<Item = ResponsePackage, Error = io::Error>>;

#[derive(Clone)]
struct MetaService {
    registry: Arc<ServiceRegistry>,
}

impl MetaService {
    pub fn new(registry: Arc<ServiceRegistry>) -> Self {
        MetaService { registry }
    }
}

impl Service for MetaService {
    type Request = RequestPackage;
    type Response = ResponsePackage;
    type Error = io::Error;
    type Future = MetaServiceFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        let (meta, controller, body) = req;
        let service = {
            let service_name = meta.get_service_name();
            let method_name = meta.get_method_name();
            self.registry
                .get_method(service_name, method_name)
                .ok_or(MethodError::UnknownError)
                .map_err(|e| {
                    warn!(
                        "Requested method {}::{} is not found",
                        service_name, method_name
                    );
                    e
                })
                .into_future()
        };
        let response = service
            .and_then(|service| service.call((body, controller)))
            .then(|resp| result_to_errno(resp));
        Box::new(response)
    }
}

impl NewService for MetaService {
    type Request = RequestPackage;
    type Response = ResponsePackage;
    type Error = io::Error;
    type Instance = Self;

    fn new_service(&self) -> io::Result<Self::Instance> {
        Ok(self.clone())
    }
}

/// Error raised when building a server
#[derive(Clone, Debug)]
pub enum ServerBuildError {
    /// Failed to parse socket address from string
    AddrParseError(AddrParseError),
}

impl fmt::Display for ServerBuildError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ServerBuildError::AddrParseError(ref e) => write!(f, "address parse error {}", e),
        }
    }
}

impl Error for ServerBuildError {
    fn description(&self) -> &str {
        match *self {
            ServerBuildError::AddrParseError(_) => "failed to parse socket address from raw string",
        }
    }

    fn cause(&self) -> Option<&Error> {
        match *self {
            ServerBuildError::AddrParseError(ref e) => Some(e),
        }
    }
}

impl From<AddrParseError> for ServerBuildError {
    fn from(e: AddrParseError) -> Self {
        ServerBuildError::AddrParseError(e)
    }
}

/// Server factory, which can be used to setup up a new server
///
/// You can chain up the methods to configure the channel.
#[derive(Debug)]
pub struct ServerBuilder<'a> {
    services: ServiceRegistry,
    addr: &'a str,
    threads: Option<usize>,
    protocols: Option<Vec<Protocol>>,
    idle_secs: Option<Second>,
    remote: Option<Remote>,
    throughput: Option<Arc<AtomicUsize>>,
}

impl<'a> ServerBuilder<'a> {
    /// Create a server listening to `addr`.
    pub fn new(addr: &'a str, services: ServiceRegistry) -> Self {
        ServerBuilder {
            services,
            addr,
            threads: None,
            protocols: None,
            idle_secs: None,
            remote: None,
            throughput: None,
        }
    }

    /// Set the number of event loops.
    ///
    /// The thread number should not exceed the CPU core number.
    /// Default to 1.
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = Some(threads);
        self
    }

    /// [WIP] Set the protocols that the server willing to support.
    pub fn protocols(mut self, protocols: Vec<Protocol>) -> Self {
        self.protocols = Some(protocols);
        self
    }

    // TODO: default to no-op
    /// Keep an idle connection for `idle` seconds before it iS shuted down
    /// by the server.
    ///
    /// Default to 60 seconds
    pub fn idle_secs(mut self, idle: Second) -> Self {
        self.idle_secs = Some(idle);
        self
    }

    /// [WIP] Server monitor, expose throught to the shared variable
    /// `throughput`.
    pub fn throughput(mut self, throughput: Arc<AtomicUsize>, remote: Remote) -> Self {
        self.throughput = Some(throughput);
        self.remote = Some(remote);
        self
    }

    /// Consume the builder and build.
    pub fn build(self) -> Result<Server, ServerBuildError> {
        let finished = Arc::new(AtomicUsize::new(0));
        let threads = self.threads.unwrap_or(1);
        let protocols = self.protocols
            .unwrap_or(vec![Protocol::Brpc, Protocol::Http]);
        let idle_secs = self.idle_secs.unwrap_or(60);
        let throughput = self.throughput.unwrap_or(Arc::new(AtomicUsize::new(0)));

        let timer = Timer::default();
        let socket_addr = self.addr.parse()?;

        let mut listener = TcpServer::new(
            MetaServerProtocol::new(protocols, timer.clone(), idle_secs, finished.clone()),
            socket_addr,
        );
        listener.threads(threads);

        info!("Server listening: {}", socket_addr);
        let server = Server {
            services: Arc::new(self.services),
            listener,
            throughput,
            finished,
            timer,
            remote: self.remote,
        };

        Ok(server)
    }
}

/// A RPC server
#[derive(Debug)]
pub struct Server {
    services: Arc<ServiceRegistry>,
    listener: TcpServer<Multiplex, MetaServerProtocol>,
    finished: Arc<AtomicUsize>,
    throughput: Arc<AtomicUsize>,
    timer: Timer,
    remote: Option<Remote>,
}

impl Server {
    /// Run the server.
    ///
    /// This method will block the current thread forever
    pub fn start(&self) {
        if let Some(ref remote) = self.remote {
            let maintainer = ThroughputMaintainer::new(
                self.timer.clone(),
                self.finished.clone(),
                self.throughput.clone(),
            );
            remote.execute(maintainer.for_each(|_| Ok(()))).unwrap();
        }

        self.listener.serve(MetaService::new(self.services.clone()))
    }
}

fn result_to_errno(
    result: Result<(Bytes, Controller), MethodError>,
) -> io::Result<ResponsePackage> {
    result
        .and_then(|(body, controller)| {
            let mut meta = RpcResponseMeta::new();
            meta.set_error_code(0);
            Ok((meta, controller, body))
        })
        .or_else(|_| {
            let mut meta = RpcResponseMeta::new();
            meta.set_error_code(1);
            meta.set_error_text("Unknown error".to_string());
            Ok((meta, Controller::default(), Bytes::new()))
        })
}
