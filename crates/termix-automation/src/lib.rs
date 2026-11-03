//! Saved command templates. This crate never executes a command by itself.

use termix_core::looks_destructive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCommand {
    pub body: String,
    pub destructive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    NeedsConfirmation,
}

pub fn prepare(body: &str, confirmed: bool) -> Result<PreparedCommand, PrepareError> {
    let destructive = looks_destructive(body);
    if destructive && !confirmed {
        return Err(PrepareError::NeedsConfirmation);
    }
    Ok(PreparedCommand {
        body: body.to_string(),
        destructive,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_requires_confirmation() {
        assert_eq!(
            prepare("rm -rf /tmp/build", false),
            Err(PrepareError::NeedsConfirmation)
        );
        assert!(prepare("rm -rf /tmp/build", true).is_ok());
        assert!(prepare("uname -a", false).is_ok());
    }
}
