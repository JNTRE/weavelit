//! Operating-system randomness for salts and session secrets.
//!
//! Randomness has no deterministic fallback here. A failure stops the operation
//! that needed it rather than producing a predictable salt or bearer value.

use crate::error::AuthenticationError;

/// Fills a fixed-size buffer from operating-system randomness.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], AuthenticationError> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|_| AuthenticationError::RandomnessUnavailable)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::random_bytes;

    #[test]
    fn randomness_fills_the_whole_buffer_with_varying_values() {
        let first: [u8; 32] = random_bytes().expect("host randomness must be available");
        let second: [u8; 32] = random_bytes().expect("host randomness must be available");
        assert_ne!(first, second);
        assert_ne!(first, [0_u8; 32]);
    }
}
