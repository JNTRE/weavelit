#![forbid(unsafe_code)]

//! SQLite implementation of the Weavelit Application Database contract.

mod connection;
mod error;
mod inspection;
mod migrations;

pub use connection::SqliteDatabase;
