//! Transport layer — IMAP/SMTP email clients.
//! Absorbed from the former abby-senses crate.

pub mod imap;
pub mod smtp;

pub use imap::ImapClient;
pub use smtp::SmtpClient;
