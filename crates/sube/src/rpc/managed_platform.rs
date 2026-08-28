//! Joinable std platform for embedded smoldot light clients.
//!
//! Smoldot's default std platform detaches its executor threads. That is fine
//! for a process-lifetime client, but not for a client living in an unloadable
//! shared library: code must stop executing before the library is unmapped.
//! This platform owns one or more executor threads through [`RuntimeGuard`],
//! whose `Drop` signals shutdown and joins every thread.
//!
//! Only raw TCP bootnodes are enabled. Smoldot still owns multistream-select,
//! Noise, Yamux, and the Substrate protocols above the socket.

use alloc::{
    borrow::Cow,
    string::{String, ToString},
    sync::{Arc, Weak},
};
use core::{
    convert::Infallible, fmt, future::Future, net::IpAddr, panic, pin::Pin, time::Duration,
};
use futures_util::{FutureExt as _, future};
use smoldot_light::platform::{
    Address, ConnectionType, LogLevel, MultiStreamAddress, MultiStreamWebRtcConnection,
    PlatformRef, SubstreamDirection, with_buffers,
};
use std::{
    io,
    net::SocketAddr,
    thread,
    time::{Instant, UNIX_EPOCH},
};

type SocketFuture = future::BoxFuture<'static, Result<smol::net::TcpStream, io::Error>>;
type Stream = with_buffers::WithBuffers<SocketFuture, smol::net::TcpStream, Instant>;

/// Cheap platform handle cloned into smoldot tasks.
///
/// The executor reference is weak on purpose: tasks must not keep the runtime
/// that owns them alive. [`RuntimeGuard`] is the sole strong owner.
#[derive(Clone)]
pub struct ManagedPlatform {
    executor: Weak<smol::Executor<'static>>,
    client_name: Arc<str>,
    client_version: Arc<str>,
}

impl panic::UnwindSafe for ManagedPlatform {}

/// Owns and synchronously shuts down all platform executor threads.
pub struct RuntimeGuard {
    executor: Arc<smol::Executor<'static>>,
    shutdown: Arc<event_listener::Event>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl RuntimeGuard {
    /// Create a platform and its owner using exactly `threads` executor
    /// threads. The returned guard must outlive every smoldot client using the
    /// platform.
    pub fn new(
        client_name: impl Into<String>,
        client_version: impl Into<String>,
        threads: usize,
    ) -> io::Result<(ManagedPlatform, Self)> {
        if threads == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "managed smoldot platform needs at least one thread",
            ));
        }

        let executor = Arc::new(smol::Executor::new());
        let shutdown = Arc::new(event_listener::Event::new());
        let mut handles = Vec::with_capacity(threads);

        for index in 0..threads {
            let listener = shutdown.listen();
            let thread_executor = Arc::clone(&executor);
            match thread::Builder::new()
                .name(format!("sube-smoldot-{index}"))
                .spawn(move || smol::block_on(thread_executor.run(listener)))
            {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    shutdown.notify(usize::MAX);
                    for handle in handles {
                        let _ = handle.join();
                    }
                    return Err(error);
                }
            }
        }

        let platform = ManagedPlatform {
            executor: Arc::downgrade(&executor),
            client_name: Arc::from(client_name.into()),
            client_version: Arc::from(client_version.into()),
        };
        let guard = RuntimeGuard {
            executor,
            shutdown,
            threads: handles,
        };
        Ok((platform, guard))
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        self.shutdown.notify(usize::MAX);
        for handle in self.threads.drain(..) {
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
            }
        }
        // Keep the executor alive through every join. It is dropped, along
        // with any cancelled tasks, immediately after this method returns.
        let _ = &self.executor;
    }
}

impl PlatformRef for ManagedPlatform {
    type Delay = futures_util::future::Map<smol::Timer, fn(Instant) -> ()>;
    type Instant = Instant;
    type MultiStream = Infallible;
    type Stream = Stream;
    type StreamConnectFuture = future::Ready<Self::Stream>;
    type MultiStreamConnectFuture = future::Pending<MultiStreamWebRtcConnection<Self::MultiStream>>;
    type ReadWriteAccess<'a> = with_buffers::ReadWriteAccess<'a, Instant>;
    type StreamUpdateFuture<'a> = future::BoxFuture<'a, ()>;
    type StreamErrorRef<'a> = &'a io::Error;
    type NextSubstreamFuture<'a> = future::Pending<Option<(Self::Stream, SubstreamDirection)>>;

    fn now_from_unix_epoch(&self) -> Duration {
        UNIX_EPOCH
            .elapsed()
            .expect("system time is before Unix epoch")
    }

    fn now(&self) -> Self::Instant {
        Instant::now()
    }

    fn fill_random_bytes(&self, buffer: &mut [u8]) {
        getrandom::fill(buffer).expect("operating-system randomness unavailable")
    }

    fn sleep(&self, duration: Duration) -> Self::Delay {
        smol::Timer::after(duration).map(|_| ())
    }

    fn sleep_until(&self, when: Self::Instant) -> Self::Delay {
        smol::Timer::at(when).map(|_| ())
    }

    fn spawn_task(&self, _task_name: Cow<str>, task: impl Future<Output = ()> + Send + 'static) {
        if let Some(executor) = self.executor.upgrade() {
            executor.spawn(task).detach();
        }
    }

    fn log<'a>(
        &self,
        level: LogLevel,
        target: &'a str,
        message: &'a str,
        key_values: impl Iterator<Item = (&'a str, &'a dyn fmt::Display)>,
    ) {
        let level = match level {
            LogLevel::Error => log::Level::Error,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Info => log::Level::Info,
            LogLevel::Debug => log::Level::Debug,
            LogLevel::Trace => log::Level::Trace,
        };
        let mut rendered = message.to_string();
        for (index, (key, value)) in key_values.enumerate() {
            use core::fmt::Write as _;
            let separator = if index == 0 { "; " } else { ", " };
            let _ = write!(rendered, "{separator}{key}={value}");
        }
        log::logger().log(
            &log::RecordBuilder::new()
                .level(level)
                .target(target)
                .args(format_args!("{rendered}"))
                .build(),
        );
    }

    fn client_name(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.client_name)
    }

    fn client_version(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.client_version)
    }

    fn supports_connection_type(&self, connection_type: ConnectionType) -> bool {
        matches!(
            connection_type,
            ConnectionType::TcpIpv4 | ConnectionType::TcpIpv6 | ConnectionType::TcpDns
        )
    }

    fn connect_stream(&self, address: Address) -> Self::StreamConnectFuture {
        enum Target {
            Socket(SocketAddr),
            Dns(String, u16),
        }

        let target = match address {
            Address::TcpDns { hostname, port } => Target::Dns(hostname.to_string(), port),
            Address::TcpIp {
                ip: IpAddr::V4(ip),
                port,
            } => Target::Socket(SocketAddr::from((ip, port))),
            Address::TcpIp {
                ip: IpAddr::V6(ip),
                port,
            } => Target::Socket(SocketAddr::from((ip, port))),
            _ => unreachable!("smoldot requested a connection type the platform rejected"),
        };

        let socket: SocketFuture = Box::pin(async move {
            let socket = match target {
                Target::Socket(address) => smol::net::TcpStream::connect(address).await?,
                Target::Dns(host, port) => smol::net::TcpStream::connect((&host[..], port)).await?,
            };
            socket.set_nodelay(true)?;
            Ok(socket)
        });
        future::ready(with_buffers::WithBuffers::new(socket))
    }

    fn connect_multistream(&self, _address: MultiStreamAddress) -> Self::MultiStreamConnectFuture {
        future::pending()
    }

    fn open_out_substream(&self, connection: &mut Self::MultiStream) {
        match *connection {}
    }

    fn next_substream(&self, connection: &mut Self::MultiStream) -> Self::NextSubstreamFuture<'_> {
        match *connection {}
    }

    fn read_write_access<'a>(
        &self,
        stream: Pin<&'a mut Self::Stream>,
    ) -> Result<Self::ReadWriteAccess<'a>, &'a io::Error> {
        stream.read_write_access(Instant::now())
    }

    fn wait_read_write_again<'a>(
        &self,
        stream: Pin<&'a mut Self::Stream>,
    ) -> Self::StreamUpdateFuture<'a> {
        Box::pin(stream.wait_read_write_again(|when| async move {
            smol::Timer::at(when).await;
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn guard_joins_threads_and_drops_pending_tasks() {
        let (platform, guard) = RuntimeGuard::new("test", "1", 1).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(Arc::clone(&dropped));
        platform.spawn_task("pending".into(), async move {
            let _flag = flag;
            started_tx.send(()).unwrap();
            future::pending::<()>().await;
        });

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("task started");
        drop(guard);
        assert!(dropped.load(Ordering::SeqCst));
        assert!(platform.executor.upgrade().is_none());
    }
}
