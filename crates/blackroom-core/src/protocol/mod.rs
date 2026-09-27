//! Message envelope, request IDs, and idempotency/duplicate/stale-request/
//! timeout primitives (Doc 16 §32–§38).

pub mod authority;
pub mod envelope;
pub mod staleness;

pub use authority::AuthorityUpdate;
pub use envelope::{Envelope, RequestId};
pub use staleness::{OperationOutcome, is_stale};
