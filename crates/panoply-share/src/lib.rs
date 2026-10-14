//! Panoply share: send and receive files between devices, LocalSend v2 compatible, and hand
//! files to any phone browser through a QR code.
//!
//! # Pieces
//!
//! - [`Node`] is a running device: an HTTPS server on the LocalSend port, multicast discovery
//!   with an HTTP scan fallback, a trust list, and an [`Event`] stream. Start one with
//!   [`Node::start`] inside a tokio runtime.
//! - Receiving is off until [`NodeConfig::receive`] is set. Incoming requests go to a
//!   [`ReceiveHandler`] (the prompt), unless the sender is trusted and the mode allows it.
//!   Files land in the chosen folder and are never overwritten.
//! - [`Node::send`] sends files and folders to a [`Device`] with progress and cancel through
//!   a [`SendJob`].
//! - [`qr::QrShare`] serves a one-time web page for devices that do not run Panoply.
//! - [`cli`] holds the `panoply share` commands.
//!
//! The wire format follows the public LocalSend protocol (v2) specification,
//! github.com/localsend/protocol. See [`proto`].

pub mod cli;
pub mod discovery;
pub mod error;
pub mod fsx;
pub mod http;
pub mod identity;
pub mod multipart;
pub mod netif;
pub mod node;
pub mod proto;
pub mod qr;
pub mod receive;
pub mod send;
pub mod trust;
pub mod words;
pub mod zip;

pub use error::{Error, Result};
pub use identity::Identity;
pub use node::{Device, Direction, Event, Node, NodeConfig, TransferEvent, TransferKind};
pub use proto::{DeviceInfo, DeviceType, Scheme};
pub use receive::{
    Decision, IncomingFile, IncomingRequest, ReceiveConfig, ReceiveHandler, ReceiveMode,
    accept_all_handler, decline_all_handler, handler_fn,
};
pub use send::{SendCancel, SendJob, SendOptions, SendSummary};
pub use trust::{TrustStore, TrustedDevice, check_code};
