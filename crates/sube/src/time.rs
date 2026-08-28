//! Unload-safe wall-clock deadlines for standard-library backends.
//!
//! Each timer owns its helper thread and synchronously joins it when the
//! timer is cancelled or dropped. Unlike process-global timer reactors, no
//! code can remain executing after an unloadable client library is destroyed.

use core::{
    future::{Future, poll_fn},
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed;

struct State {
    cancelled: bool,
    fired: bool,
    waker: Option<Waker>,
}

struct JoinableTimer {
    state: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl JoinableTimer {
    fn new(duration: Duration) -> Self {
        let state = Arc::new((
            Mutex::new(State {
                cancelled: false,
                fired: false,
                waker: None,
            }),
            Condvar::new(),
        ));
        let thread_state = Arc::clone(&state);
        let thread = thread::Builder::new()
            .name("sube-deadline".into())
            .spawn(move || {
                let (lock, wake) = &*thread_state;
                let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let (mut guard, _) = wake
                    .wait_timeout_while(guard, duration, |state| !state.cancelled)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if !guard.cancelled {
                    guard.fired = true;
                    if let Some(waker) = guard.waker.take() {
                        drop(guard);
                        waker.wake();
                    }
                }
            })
            .expect("failed to spawn joinable deadline thread");
        Self {
            state,
            thread: Some(thread),
        }
    }
}

impl Future for JoinableTimer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let (lock, _) = &*self.state;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.fired {
            Poll::Ready(())
        } else {
            if state
                .waker
                .as_ref()
                .is_none_or(|waker| !waker.will_wake(cx.waker()))
            {
                state.waker = Some(cx.waker().clone());
            }
            Poll::Pending
        }
    }
}

impl Drop for JoinableTimer {
    fn drop(&mut self) {
        let (lock, wake) = &*self.state;
        {
            let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.cancelled = true;
            state.waker = None;
            wake.notify_all();
        }
        if let Some(timer_thread) = self.thread.take()
            && timer_thread.thread().id() != thread::current().id()
        {
            let _ = timer_thread.join();
        }
    }
}

/// Run `future` until it completes or the wall-clock deadline expires.
/// Dropping either branch synchronously stops and joins its timer thread.
pub async fn timeout<T>(duration: Duration, future: impl Future<Output = T>) -> Result<T, Elapsed> {
    let mut future = core::pin::pin!(future);
    let mut timer = core::pin::pin!(JoinableTimer::new(duration));
    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(Elapsed));
        }
        Poll::Pending
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::future;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Dropped(Arc<AtomicBool>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn timeout_cancels_the_future_and_joins_its_timer() {
        let dropped = Arc::new(AtomicBool::new(false));
        let marker = Dropped(Arc::clone(&dropped));
        let result = smol::block_on(timeout(Duration::from_millis(10), async move {
            let _marker = marker;
            future::pending::<()>().await;
        }));
        assert_eq!(result, Err(Elapsed));
        assert!(dropped.load(Ordering::SeqCst));
    }
}
