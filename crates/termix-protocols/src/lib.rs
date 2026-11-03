//! Protocols that do not have a working backend return an error.
//! They are not advertised as connected.

use termix_core::{Capability, Support};

#[derive(Debug, thiserror::Error)]
#[error("{capability} is not implemented")]
pub struct NotImplemented {
    pub capability: &'static str,
}

pub fn require(capability: Capability) -> Result<(), NotImplemented> {
    match capability.support() {
        Support::Implemented => Ok(()),
        Support::NotImplemented => Err(NotImplemented {
            capability: capability.label(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdp_is_not_implemented() {
        assert!(require(Capability::Rdp).is_err());
        assert!(require(Capability::Ssh).is_ok());
    }
}
