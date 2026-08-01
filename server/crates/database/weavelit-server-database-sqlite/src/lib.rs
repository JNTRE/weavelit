#![forbid(unsafe_code)]

//! SQLite implementation of the Weavelit Application Database contract.

mod connection;
mod error;

pub use connection::SqliteDatabase;
