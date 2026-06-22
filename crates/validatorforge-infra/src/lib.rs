//! Infrastructure adapters: concrete implementations of the `validatorforge-core`
//! ports. The default build ships in-memory adapters (suitable for the simulator
//! and tests); enabling the `postgres` feature adds a range-partitioned SQL
//! repository for durable run history.

#![forbid(unsafe_code)]

mod agent;
mod clock;
mod events;
mod iac;
mod repo;

#[cfg(feature = "postgres")]
mod pg;

pub use agent::SimNodeAgent;
pub use clock::UtcClock;
pub use events::BroadcastEventSink;
pub use iac::DefaultIacRenderer;
pub use repo::InMemoryNodeRepository;

#[cfg(feature = "postgres")]
pub use pg::PgNodeRepository;
