use mainframe_env_store_api::StoreError;
use std::future::Future;
use tokio::runtime::{Handle, Runtime, RuntimeFlavor};

pub(crate) struct AdapterRuntime {
    inner: Option<Runtime>,
}

impl AdapterRuntime {
    pub(crate) fn new(runtime: Runtime) -> Self {
        Self {
            inner: Some(runtime),
        }
    }

    fn get(&self) -> &Runtime {
        self.inner.as_ref().expect("adapter runtime is live")
    }
}

impl Drop for AdapterRuntime {
    fn drop(&mut self) {
        let Some(runtime) = self.inner.take() else {
            return;
        };
        if Handle::try_current().is_ok() {
            // Dropping a Tokio runtime may wait for blocking tasks and is
            // therefore forbidden inside an async context. Background shutdown
            // consumes the adapter runtime without blocking the async worker.
            runtime.shutdown_background();
        } else {
            drop(runtime);
        }
    }
}

/// Runs SQL adapter work without nesting a Tokio runtime on an async worker.
///
/// The owned store contracts are synchronous in 0.1. Production callers can
/// therefore reach them from either ordinary threads or Tokio tasks. On a
/// multi-thread runtime Tokio's blocking lane is used; on a current-thread
/// runtime a scoped OS thread isolates the adapter runtime.
pub(crate) fn block_on<F>(runtime: &AdapterRuntime, future: F) -> Result<F::Output, StoreError>
where
    F: Future + Send,
    F::Output: Send,
{
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            Ok(tokio::task::block_in_place(|| {
                runtime.get().block_on(future)
            }))
        }
        Ok(_) => std::thread::scope(|scope| {
            scope
                .spawn(move || runtime.get().block_on(future))
                .join()
                .map_err(|_| StoreError::Infrastructure("SQL adapter worker panicked".into()))
        }),
        Err(_) => Ok(runtime.get().block_on(future)),
    }
}
