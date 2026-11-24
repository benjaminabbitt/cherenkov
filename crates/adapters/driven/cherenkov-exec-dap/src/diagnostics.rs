//! The adapter's errors are coded, with named data, never English: the
//! messages are `locales/en-US/messages.ftl`'s.

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every error, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn angzarr_diagnostic::Diagnostic>> {
    use crate::isa::IsaError;
    vec![
        Box::new(IsaError::Toml { detail: "d".into() }),
        Box::new(IsaError::UnknownToken { name: "n".into() }),
        Box::new(IsaError::UnknownCondition { name: "n".into() }),
        Box::new(IsaError::BadDisplacement { rel: 2 }),
        Box::new(IsaError::NoStub { abi: "vax".into() }),
        Box::new(IsaError::BadStubLine { line: "l".into() }),
    ]
}

#[cfg(test)]
#[path = "diagnostics.test.rs"]
mod tests;
