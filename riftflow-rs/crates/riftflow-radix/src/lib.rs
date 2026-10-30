//! Efficient URI-pattern event fan-out for LCU subscriptions.
//!
//! Ports of `src/shared/utils/radix-matcher.ts` and
//! `src/shared/utils/event-emitter.ts` (`RadixEventEmitter`).

mod emitter;
mod matcher;

pub use emitter::{Listener, RadixEventEmitter, Subscription};
pub use matcher::{infer_node_type, FindResult, NodeType, RadixMatcher};
