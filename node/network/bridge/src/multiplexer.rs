// Copyright 2021 Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.

use std::pin::Pin;
use std::unreachable;

use futures::channel::mpsc;
use futures::stream::{FusedStream, Stream};
use futures::task::{Context, Poll};
use strum::IntoEnumIterator;

use parity_scale_codec::{Decode, Error as DecodingError};

use sc_network::config as network;
use sc_network::PeerId;

use polkadot_node_network_protocol::request_response::{
	request::IncomingRequest, v1, Protocol, RequestResponseConfig,
};
use polkadot_overseer::AllMessages;

/// Multiplex incoming network requests.
///
/// This multiplexer consumes all request streams and makes them a `Stream` of a single message
/// type, useful for the network bridge to send them via the `Overseer` to other subsystems.
///
/// The resulting stream will end once any of its input ends.
///
/// TODO: Get rid of this: https://github.com/paritytech/polkadot/issues/2842
pub struct RequestMultiplexer {
	receivers: Vec<(Protocol, mpsc::Receiver<network::IncomingRequest>)>,
	statement_fetching: Option<mpsc::Receiver<network::IncomingRequest>>,
	dispute_sending: Option<mpsc::Receiver<network::IncomingRequest>>,
	next_poll: usize,
}

/// Multiplexing can fail in case of invalid messages.
#[derive(Debug, PartialEq, Eq)]
pub struct RequestMultiplexError {
	/// The peer that sent the invalid message.
	pub peer: PeerId,
	/// The error that occurred.
	pub error: DecodingError,
}

impl RequestMultiplexer {
	/// Create a new `RequestMultiplexer`.
	///
	/// This function uses `Protocol::get_config` for each available protocol and creates a
	/// `RequestMultiplexer` from it. The returned `RequestResponseConfig`s must be passed to the
	/// network implementation.
	pub fn new() -> (Self, Vec<RequestResponseConfig>) {
		let (mut receivers, cfgs): (Vec<_>, Vec<_>) = Protocol::iter()
			.map(|p| {
				let (rx, cfg) = p.get_config();
				((p, rx), cfg)
			})
			.unzip();

		// Ok this code is ugly as hell, it is also a hack, see https://github.com/paritytech/polkadot/issues/2842.
		// But it works and is executed on startup so, if anything is wrong here it will be noticed immediately.
		let index = receivers.iter().enumerate().find_map(|(i, (p, _))|
			if let Protocol::StatementFetching = p {
				Some(i)
			} else {
				None
			}
		).expect("Statement fetching must be registered. qed.");
		let statement_fetching = Some(receivers.remove(index).1);

		let index = receivers.iter().enumerate().find_map(|(i, (p, _))|
			if let Protocol::DisputeSending = p {
				Some(i)
			} else {
				None
			}
		).expect("Dispute sending must be registered. qed.");
		let dispute_sending = Some(receivers.remove(index).1);

		(
			Self {
				receivers,
				statement_fetching,
                dispute_sending,
				next_poll: 0,
			},
			cfgs,
		)
	}

	/// Get the receiver for handling statement fetching requests.
	///
	/// This function will only return `Some` once.
	pub fn get_statement_fetching(&mut self) -> Option<mpsc::Receiver<network::IncomingRequest>> {
		std::mem::take(&mut self.statement_fetching)
	}

	/// Get the receiver for handling dispute sending requests.
	///
	/// This function will only return `Some` once.
	pub fn get_dispute_sending(&mut self) -> Option<mpsc::Receiver<network::IncomingRequest>> {
		std::mem::take(&mut self.dispute_sending)
	}
}

impl Stream for RequestMultiplexer {
	type Item = Result<AllMessages, RequestMultiplexError>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let len = self.receivers.len();
		let mut count = len;
		let mut i = self.next_poll;
		let mut result = Poll::Ready(None);
		// Poll streams in round robin fashion:
		while count > 0 {
			// % safe, because count initialized to len, loop would not be entered if 0, also
			// length of receivers is fixed.
			let (p, rx): &mut (_, _) = &mut self.receivers[i % len];
			// Avoid panic:
			if rx.is_terminated() {
				// Early return, we don't want to update next_poll.
				return Poll::Ready(None);
			}
			i += 1;
			count -= 1;
			match Pin::new(rx).poll_next(cx) {
				Poll::Pending => result = Poll::Pending,
				// We are done, once a single receiver is done.
				Poll::Ready(None) => return Poll::Ready(None),
				Poll::Ready(Some(v)) => {
					result = Poll::Ready(Some(multiplex_single(*p, v)));
					break;
				}
			}
		}
		self.next_poll = i;
		result
	}
}

impl FusedStream for RequestMultiplexer {
	fn is_terminated(&self) -> bool {
		let len = self.receivers.len();
		if len == 0 {
			return true;
		}
		let (_, rx) = &self.receivers[self.next_poll % len];
		rx.is_terminated()
	}
}

/// Convert a single raw incoming request into a `MultiplexMessage`.
fn multiplex_single(
	p: Protocol,
	network::IncomingRequest {
		payload,
		peer,
		pending_response,
	}: network::IncomingRequest,
) -> Result<AllMessages, RequestMultiplexError> {
	let r = match p {
		Protocol::ChunkFetching => AllMessages::from(IncomingRequest::new(
			peer,
			decode_with_peer::<v1::ChunkFetchingRequest>(peer, payload)?,
			pending_response,
		)),
		Protocol::CollationFetching => AllMessages::from(IncomingRequest::new(
			peer,
			decode_with_peer::<v1::CollationFetchingRequest>(peer, payload)?,
			pending_response,
		)),
		Protocol::PoVFetching => AllMessages::from(IncomingRequest::new(
			peer,
			decode_with_peer::<v1::PoVFetchingRequest>(peer, payload)?,
			pending_response,
		)),
		Protocol::AvailableDataFetching => AllMessages::from(IncomingRequest::new(
			peer,
			decode_with_peer::<v1::AvailableDataFetchingRequest>(peer, payload)?,
			pending_response,
		)),
		Protocol::StatementFetching => {
			unreachable!("Statement fetching requests are handled directly. qed.");
		}
		Protocol::DisputeSending => {
			unreachable!("Dispute sending request are handled directly. qed.");
		}
	};
	Ok(r)
}

fn decode_with_peer<Req: Decode>(
	peer: PeerId,
	payload: Vec<u8>,
) -> Result<Req, RequestMultiplexError> {
	Req::decode(&mut payload.as_ref()).map_err(|error| RequestMultiplexError { peer, error })
}

#[cfg(test)]
mod tests {
	use futures::prelude::*;
	use futures::stream::FusedStream;

	use super::RequestMultiplexer;
	#[test]
	fn check_exhaustion_safety() {
		// Create and end streams:
		fn drop_configs() -> RequestMultiplexer {
			let (multiplexer, _) = RequestMultiplexer::new();
			multiplexer
		}
		let multiplexer = drop_configs();
		futures::executor::block_on(async move {
			let mut f = multiplexer;
			assert!(f.next().await.is_none());
			assert!(f.is_terminated());
			assert!(f.next().await.is_none());
			assert!(f.is_terminated());
			assert!(f.next().await.is_none());
			assert!(f.is_terminated());
		});
	}
}
