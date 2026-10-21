//! Local access contract for the Hive control plane.
//!
//! The Hive daemon keeps a per-install control token in its data root. Every
//! Hive route except `/health`, the adoption challenge, and runtime lease
//! issuance requires that token or an Entity runtime lease as a bearer. A
//! desktop shell must never send the token to a daemon until the daemon has
//! proved it holds the same token for the expected channel and URL.

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// File in the Hive data root holding the control token.
pub const CONTROL_TOKEN_FILE: &str = "hive-control.token";

/// Unauthenticated route that answers an [`AdoptionChallenge`].
pub const ADOPTION_PATH: &str = "/v1/hive/adoption";

const TOKEN_LEN: usize = 64;
const NONCE_LEN: usize = 64;
const ADOPTION_CONTEXT: &[u8] = b"abigail-hive-adoption-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptionChallenge {
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptionProof {
    pub channel: String,
    pub proof: String,
}

/// The token stored in a control-token file, if the contents are well formed.
pub fn parse_control_token(contents: &str) -> Option<&str> {
    let token = contents.trim();
    is_hex(token, TOKEN_LEN).then_some(token)
}

/// Adoption nonces are 32 random bytes, hex encoded.
pub fn is_valid_nonce(nonce: &str) -> bool {
    is_hex(nonce, NONCE_LEN)
}

/// Compares a presented bearer with the control token without an early exit.
pub fn tokens_match(presented: &str, expected: &str) -> bool {
    presented.len() == expected.len()
        && presented
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

/// Proof that a daemon listening at `hive_url` for `channel` holds `token`.
/// Binding the URL means a relay listening elsewhere cannot replay a proof.
pub fn adoption_proof(token: &str, channel: &str, hive_url: &str, nonce: &str) -> String {
    let bytes = adoption_mac(token, channel, hive_url, nonce)
        .finalize()
        .into_bytes();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn verify_adoption_proof(
    token: &str,
    channel: &str,
    hive_url: &str,
    nonce: &str,
    proof: &str,
) -> bool {
    let Some(proof) = decode_hex(proof) else {
        return false;
    };
    adoption_mac(token, channel, hive_url, nonce)
        .verify_slice(&proof)
        .is_ok()
}

fn adoption_mac(token: &str, channel: &str, hive_url: &str, nonce: &str) -> Hmac<Sha256> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC accepts keys of any length");
    for part in [
        ADOPTION_CONTEXT,
        channel.as_bytes(),
        hive_url.as_bytes(),
        nonce.as_bytes(),
    ] {
        mac.update(&(part.len() as u64).to_be_bytes());
        mac.update(part);
    }
    mac
}

/// Whether a `Host` header names this machine's loopback interface. Browsers
/// send the page's own hostname, so this rejects DNS-rebinding requests.
pub fn is_loopback_host(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(bracketed) => match bracketed.split_once(']') {
            Some((address, "")) => address,
            Some((address, rest)) if rest.strip_prefix(':').is_some_and(is_port) => address,
            _ => return false,
        },
        None => match host.split_once(':') {
            Some((name, port)) if is_port(port) => name,
            Some(_) => return false,
            None => host,
        },
    };
    name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1" || name == "::1"
}

fn is_port(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u16>().is_ok()
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const NONCE: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
    const URL: &str = "http://127.0.0.1:43141";

    #[test]
    fn control_tokens_must_be_64_hex_characters() {
        assert_eq!(parse_control_token(&format!(" {TOKEN}\n")), Some(TOKEN));
        assert_eq!(parse_control_token(&TOKEN[1..]), None);
        assert_eq!(parse_control_token(&format!("{TOKEN}0")), None);
        assert_eq!(parse_control_token(&TOKEN.replace('a', "g")), None);
        assert_eq!(parse_control_token(""), None);
        assert!(is_valid_nonce(NONCE));
        assert!(!is_valid_nonce("short"));
    }

    #[test]
    fn token_comparison_rejects_prefixes_and_near_misses() {
        assert!(tokens_match(TOKEN, TOKEN));
        assert!(!tokens_match(&TOKEN[..63], TOKEN));
        assert!(!tokens_match(&TOKEN.replace('f', "e"), TOKEN));
        assert!(!tokens_match("", TOKEN));
    }

    #[test]
    fn adoption_proof_binds_token_channel_url_and_nonce() {
        let proof = adoption_proof(TOKEN, "stable", URL, NONCE);
        assert_eq!(proof.len(), 64);
        assert!(verify_adoption_proof(TOKEN, "stable", URL, NONCE, &proof));

        let other_token = TOKEN.replace('0', "1");
        let other_nonce = NONCE.replace('f', "e");
        for (token, channel, url, nonce) in [
            (other_token.as_str(), "stable", URL, NONCE),
            (TOKEN, "preview", URL, NONCE),
            (TOKEN, "stable", "http://127.0.0.1:43142", NONCE),
            (TOKEN, "stable", URL, other_nonce.as_str()),
        ] {
            assert!(!verify_adoption_proof(token, channel, url, nonce, &proof));
        }
        assert!(!verify_adoption_proof(
            TOKEN,
            "stable",
            URL,
            NONCE,
            &proof[..62]
        ));
        assert!(!verify_adoption_proof(TOKEN, "stable", URL, NONCE, "zz"));
        assert!(!verify_adoption_proof(TOKEN, "stable", URL, NONCE, ""));
    }

    #[test]
    fn proof_fields_cannot_be_shifted_into_each_other() {
        assert_ne!(
            adoption_proof(TOKEN, "stable", "ab", NONCE),
            adoption_proof(TOKEN, "stablea", "b", NONCE)
        );
    }

    #[test]
    fn only_loopback_host_headers_are_local() {
        for host in [
            "127.0.0.1",
            "127.0.0.1:43141",
            "localhost:43141",
            "LOCALHOST",
            "[::1]",
            "[::1]:43141",
        ] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in [
            "",
            "attacker.example:43141",
            "127.0.0.1.attacker.example",
            "localhost.attacker.example:43141",
            "127.0.0.2:43141",
            "0.0.0.0:43141",
            "127.0.0.1:",
            "127.0.0.1:99999",
            "127.0.0.1:43141:1",
            "[::1",
            "[::1]x",
            "[::2]:43141",
            "user@127.0.0.1",
        ] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }
}
