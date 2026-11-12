use tokio::sync::mpsc;

/// Transient host notifications, independent of JSON result documents.
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// A host is resolving metadata or fetching a dependency.
    Preparing {
        /// Human-readable operation, excluded from result documents.
        message: String,
    },
    /// A step is about to run, including hooks.
    Started {
        /// Workflow-local step identifier.
        id: String,
    },
    /// A step completed successfully.
    Completed {
        /// Workflow-local step identifier.
        id: String,
    },
    /// Process output forwarded to the host's diagnostic stream.
    Log {
        /// Unmodified child output bytes.
        bytes: Vec<u8>,
    },
}

/// Optional native-only event sink; it is never serialized into Wasm contexts.
#[derive(Debug, Clone, Default)]
pub struct EventSink {
    sender: Option<mpsc::Sender<ExecutionEvent>>,
}

/// Bounded event stream for an application's own terminal or logging adapter.
#[derive(Debug)]
pub struct EventStream {
    receiver: mpsc::Receiver<ExecutionEvent>,
}

impl EventSink {
    /// Create a bounded channel; process logs apply backpressure to producers.
    #[must_use]
    pub fn channel() -> (Self, EventStream) {
        let (sender, receiver) = mpsc::channel(64);
        (
            Self {
                sender: Some(sender),
            },
            EventStream { receiver },
        )
    }

    /// Whether a native host is observing execution.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.sender.is_some()
    }

    pub(crate) fn emit(&self, event: ExecutionEvent) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(event);
        }
    }

    /// Report dependency preparation without changing the result protocol.
    pub fn preparing(&self, message: impl Into<String>) {
        self.emit(ExecutionEvent::Preparing {
            message: message.into(),
        });
    }

    /// Forward a bounded chunk of process output, waiting for a slow consumer.
    pub async fn log(&self, bytes: Vec<u8>) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(ExecutionEvent::Log { bytes }).await;
        }
    }
}

impl EventStream {
    /// Wait for the next native notification.
    pub async fn next(&mut self) -> Option<ExecutionEvent> {
        self.receiver.recv().await
    }
    /// Read a queued notification without waiting.
    pub fn try_next(&mut self) -> Option<ExecutionEvent> {
        self.receiver.try_recv().ok()
    }
}
