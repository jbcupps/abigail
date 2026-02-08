//! Transport layer — IMAP/SMTP email clients.
//! Absorbed from the former abigail-senses crate.

pub mod imap;
pub mod smtp;

pub use imap::ImapClient;
pub use smtp::SmtpClient;
