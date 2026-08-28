//! Joinable std platform for embedded smoldot light clients.
//!
//! The entire executor, timer, DNS, and socket reactor belongs to one Tokio
//! runtime. Dropping [`RuntimeGuard`] first prevents new tasks from spawning,
//! then shuts down and joins that runtime before an unloadable library can be
//! unmapped. No process-global async driver is used.

use alloc::{
    borrow::Cow,
    rc::Rc,
    string::{String, ToString},
    sync::{Arc, Weak},
};
use core::{
    convert::Infallible, fmt, future::Future, marker::PhantomData, net::IpAddr, panic, pin::Pin,
    time::Duration,
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
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt as _};

type Socket = Compat<tokio::net::TcpStream>;
type SocketFuture = future::BoxFuture<'static, Result<Socket, io::Error>>;
type Stream = with_buffers::WithBuffers<SocketFuture, Socket, Instant>;

#[derive(Clone)]
pub struct ManagedPlatform {
    runtime: tokio::runtime::Handle,
    alive: Weak<()>,
    client_name: Arc<str>,
    client_version: Arc<str>,
}

impl panic::UnwindSafe for ManagedPlatform {}

/// Owns and synchronously shuts down all platform facilities and threads.
pub struct RuntimeGuard {
    alive: Option<Arc<()>>,
    runtime: Option<tokio::runtime::Runtime>,
    // A guard cannot be moved onto one of the Send tasks it owns: joining an
    // executor from its own worker would be logically impossible.
    _not_send: PhantomData<Rc<()>>,
}

impl RuntimeGuard {
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

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads)
            .thread_name("sube-smoldot")
            .enable_io()
            .enable_time()
            .build()?;
        let alive = Arc::new(());
        let platform = ManagedPlatform {
            runtime: runtime.handle().clone(),
            alive: Arc::downgrade(&alive),
            client_name: Arc::from(client_name.into()),
            client_version: Arc::from(client_version.into()),
        };
        Ok((
            platform,
            Self {
                alive: Some(alive),
                runtime: Some(runtime),
                _not_send: PhantomData,
            },
        ))
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        // Refuse new spawns before Runtime::drop cancels tasks and joins every
        // worker, I/O driver, timer driver, and blocking worker it owns.
        self.alive.take();
        let Some(runtime) = self.runtime.take() else {
            return;
        };

        // Tokio deliberately rejects blocking Runtime::drop from within an
        // async context. Destruction on a short-lived helper keeps Sube safe
        // to use from a foreign Tokio application while still joining every
        // owned thread before this guard returns.
        let shutdown = thread::Builder::new()
            .name("sube-smoldot-shutdown".into())
            .spawn(move || drop(runtime))
            .expect("failed to spawn smoldot runtime shutdown thread");
        shutdown
            .join()
            .expect("smoldot runtime shutdown thread panicked");
    }
}

impl PlatformRef for ManagedPlatform {
    type Delay = future::BoxFuture<'static, ()>;
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
        Box::pin(async move { tokio::time::sleep(duration).await })
    }

    fn sleep_until(&self, when: Self::Instant) -> Self::Delay {
        Box::pin(async move { tokio::time::sleep_until(when.into()).await })
    }

    fn spawn_task(&self, _task_name: Cow<str>, task: impl Future<Output = ()> + Send + 'static) {
        if self.alive.upgrade().is_some() {
            drop(
                self.runtime
                    .spawn(panic::AssertUnwindSafe(task).catch_unwind().map(|_| ())),
            );
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
                Target::Socket(address) => tokio::net::TcpStream::connect(address).await?,
                Target::Dns(host, port) => {
                    tokio::net::TcpStream::connect((&host[..], port)).await?
                }
            };
            socket.set_nodelay(true)?;
            Ok(socket.compat())
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
            tokio::time::sleep_until(when.into()).await;
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
    fn guard_joins_runtime_after_timer_and_pending_task() {
        let (platform, guard) = RuntimeGuard::new("test", "1", 1).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(Arc::clone(&dropped));
        platform.spawn_task("pending".into(), async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            let _flag = flag;
            started_tx.send(()).unwrap();
            future::pending::<()>().await;
        });

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("timer-backed task started");
        drop(guard);
        assert!(dropped.load(Ordering::SeqCst));
        assert!(platform.alive.upgrade().is_none());
    }

    #[test]
    fn guard_can_be_dropped_from_a_foreign_tokio_context() {
        let outer = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        outer.block_on(async {
            let (_platform, guard) = RuntimeGuard::new("test", "1", 1).unwrap();
            tokio::task::yield_now().await;
            drop(guard);
        });
    }
}
