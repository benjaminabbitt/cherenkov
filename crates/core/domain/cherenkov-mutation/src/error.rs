//! The domain's errors are coded, with named data, never English: the
//! messages are `locales/en-US/messages.ftl`'s. The errors are
//! [`crate::token::TokenTableError`] and [`crate::set_spec::SetError`].

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every error, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn angzarr_diagnostic::Diagnostic>> {
    use crate::set_spec::SetError;
    use crate::token::TokenTableError;
    use angzarr_diagnostic::Report;
    vec![
        Box::new(TokenTableError::BadNotTemplate {
            template: "t".into(),
        }),
        Box::new(TokenTableError::DuplicateSpelling {
            spelling: "&".into(),
        }),
        Box::new(SetError::Unknown { name: "n".into() }),
        Box::new(SetError::Unreadable {
            path: "p".into(),
            detail: "d".into(),
        }),
        Box::new(SetError::Invalid {
            path: "p".into(),
            cause: Report::new("cherenkov-mutation-unknown-skip").arg("name", "n"),
        }),
        Box::new(SetError::UnknownMutation { name: "n".into() }),
        Box::new(SetError::Alias {
            alias: "a".into(),
            canonical: "b, c".into(),
        }),
        Box::new(SetError::UnknownSkip { name: "n".into() }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
