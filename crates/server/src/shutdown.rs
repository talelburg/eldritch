//! Graceful shutdown: one trigger that tells every open game socket to close,
//! and a count of the sockets still open, so the process can wait for them.
//!
//! Axum's graceful shutdown waits on HTTP connections. An upgraded socket's
//! task isn't one of those, so the sockets are tracked here instead.

use tokio::sync::watch;

/// The shutdown trigger and the open sockets it closes.
///
/// Clones share one trigger. `main` fires it on SIGINT or SIGTERM, and tests
/// fire it through the handle they gave [`AppState`](crate::AppState).
#[derive(Clone, Debug)]
pub struct Shutdown {
    /// `true` once shutdown is triggered. Never set back.
    triggered: watch::Sender<bool>,
    /// Each open socket holds one receiver, so the sender is closed exactly
    /// when no socket is open.
    sockets: watch::Sender<()>,
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl Shutdown {
    /// A trigger not yet fired, with no socket open.
    #[must_use]
    pub fn new() -> Self {
        Self {
            triggered: watch::Sender::new(false),
            sockets: watch::Sender::new(()),
        }
    }

    /// Tell every open socket to close, and the server to stop accepting
    /// connections. Firing it again does nothing more.
    pub fn trigger(&self) {
        self.triggered.send_replace(true);
    }

    /// Resolve once [`trigger`](Self::trigger) has fired, at once if it
    /// already has.
    pub async fn triggered(&self) {
        let mut triggered = self.triggered.subscribe();
        // The sender lives in `self`, so the wait can't fail on a closed
        // channel.
        let _ = triggered.wait_for(|fired| *fired).await;
    }

    /// Resolve once no game socket is open: at once if none is, else when
    /// the last open one closes.
    pub async fn sockets_closed(&self) {
        self.sockets.closed().await;
    }

    /// Count one socket as open until the returned guard drops.
    pub(crate) fn track(&self) -> SocketGuard {
        SocketGuard {
            _open: self.sockets.subscribe(),
        }
    }
}

/// One open socket, counted by its [`Shutdown`] until this drops.
pub(crate) struct SocketGuard {
    _open: watch::Receiver<()>,
}
