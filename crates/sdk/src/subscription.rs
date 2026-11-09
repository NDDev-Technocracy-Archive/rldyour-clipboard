use crate::{Client, Error, policy::Policy};
use nddev_clipboard_api::wire::{Response, Role};
use std::io::{Read, Write};
use std::sync::Arc;

/// A dedicated v1 notification connection. It exposes no request methods, so
/// broadcasts cannot be mistaken for a serialized request's acknowledgement.
/// Caller-owned transports must supply their own I/O lifetime; native callers
/// use `LocalOperation::subscribe` for an absolute deadline and cancellation.
pub struct Subscription<T> {
    client: Client<T>,
    greeting: Response,
}

impl<T: Read + Write> Subscription<T> {
    pub fn new(transport: T) -> Result<Self, Error> {
        Self::with_policy(transport, None)
    }

    pub(crate) fn with_policy(transport: T, policy: Option<Arc<Policy>>) -> Result<Self, Error> {
        let (client, greeting) = Client::negotiate(transport, Role::Ui, true, policy)?;
        Ok(Self { client, greeting })
    }

    /// The validated v1 Hello, including the provider's current archive policy.
    pub fn greeting(&self) -> &Response {
        &self.greeting
    }

    pub fn next_event(&mut self) -> Result<Response, Error> {
        match self.client.frame()? {
            event @ (Response::Added { .. }
            | Response::Updated { .. }
            | Response::Removed { .. }
            | Response::Cleared {}) => Ok(event),
            _ => self.client.invalidate(Error::UnexpectedResponse),
        }
    }
}
