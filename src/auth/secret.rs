//! Secure handling of private keys and secrets.
//!
//! This module provides mechanisms to load sensitive data (like private keys)
//! without exposing them in logs or keeping them unprotected in memory.
//!
//! # Features
//! - Loads from STDIN (preferred for manual/secure starts)
//! - Loads from Environment Variables (fallback for automation)
//! - Uses `secrecy` crate to prevent accidental logging (Debug trait redacts content)
//! - Uses `zeroize` to clear memory on drop

use crate::error::{Error, Result};
use std::env;
use std::io::{self, BufRead};
use zeroize::Zeroizing;
use std::fmt;

/// A wrapper for sensitive private key data.
/// Wraps `Zeroizing<String>` to ensure memory is cleared on drop.
/// Implements `Debug` to print "REDACTED" instead of the key.
#[derive(Clone, Default)]
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(s: String) -> Self {
        Self(Zeroizing::new(s))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "REDACTED")
    }
}

pub type PrivateKey = SecretString;

/// Loader for secure secrets.
pub struct SecretLoader;

impl SecretLoader {
    /// Attempt to load the private key from multiple sources in order of security priority.
    ///
    /// Priority:
    /// 1. STDIN (if piped or interactive) - Most secure, no disk trace.
    /// 2. `OPINION_PRIVATE_KEY` environment variable - Standard for container orchestration.
    ///
    /// # Returns
    /// * `Ok(PrivateKey)` - The loaded key wrapped in a Secret type.
    /// * `Err(Error)` - If no key could be found.
    pub fn load_private_key() -> Result<PrivateKey> {
        // 1. Check Env var first (non-blocking check)
        if let Ok(key) = env::var("OPINION_PRIVATE_KEY") {
            if !key.trim().is_empty() {
                return Ok(SecretString::new(clean_key(&key)));
            }
        }

        // 2. Fallback to STDIN (blocking)
        eprintln!("Authentication: Reading private key from STDIN...");
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut line = String::new();
        
        match reader.read_line(&mut line) {
            Ok(n) if n > 0 => {
                let cleaned = clean_key(&line);
                if cleaned.is_empty() {
                     return Err(Error::ConfigError("Empty private key provided via STDIN".into()));
                }
                if cleaned.len() != 64 {
                    return Err(Error::ConfigError("Invalid private key length from STDIN".into()));
                }
                Ok(SecretString::new(cleaned))
            }
            _ => Err(Error::MissingConfig { 
                field: "OPINION_PRIVATE_KEY or STDIN".into() 
            }),
        }
    }
}

/// Helper to strip whitespace and '0x' prefix.
fn clean_key(input: &str) -> String {
    let trimmed = input.trim();
    trimmed.strip_prefix("0x").unwrap_or(trimmed).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_key() {
        assert_eq!(clean_key("0x123abc"), "123abc");
        assert_eq!(clean_key("  123abc  "), "123abc");
        assert_eq!(clean_key("  0x123abc  "), "123abc");
    }

    #[test]
    fn test_secret_debug() {
        let secret = SecretString::new("my_secret".to_string());
        assert_eq!(format!("{:?}", secret), "REDACTED");
        assert_eq!(secret.expose_secret(), "my_secret");
    }
}
