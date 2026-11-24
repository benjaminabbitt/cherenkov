//! A failure's reason between [`Report`] and the wire's `Diagnostic`.
//!
//! angzarr flattens a diagnostic's args onto the wire (decision 14.37): each
//! arg by its name, a nested cause as its code under the arg's name and each
//! of its args under `<name>.<arg>`. angzarr-embedded's own converter is
//! private to it, so the run layer's is here, and reads the flat form back
//! into nested causes. A cause with no args would leave the flat form no
//! different from a text arg that holds a code, so it also writes an arg
//! named `<name>.` (an empty arg name under it): the one mark of a cause.

use std::collections::BTreeMap;

use coded_diagnostic::{Arg, Diagnostic, Report};

use crate::angzarr;
use crate::failure::Failure;

/// `report` as the wire holds it: its code and its flattened args.
pub fn encode(report: &Report) -> angzarr::Diagnostic {
    angzarr::Diagnostic {
        code: report.code().to_string(),
        args: flattened(report).into_iter().collect(),
    }
}

fn flattened(diagnostic: &dyn Diagnostic) -> Vec<(String, String)> {
    let mut flat = Vec::new();
    for (name, arg) in diagnostic.args() {
        match arg {
            Arg::Int(n) => flat.push((name.to_string(), n.to_string())),
            Arg::Uint(n) => flat.push((name.to_string(), n.to_string())),
            Arg::Str(s) => flat.push((name.to_string(), s)),
            Arg::Nested(cause) => {
                flat.push((name.to_string(), cause.code().to_string()));
                flat.push((format!("{name}."), String::new()));
                flat.extend(
                    flattened(cause.as_ref())
                        .into_iter()
                        .map(|(inner, value)| (format!("{name}.{inner}"), value)),
                );
            }
        }
    }
    flat
}

/// The reason a record holds: `why` read back as a report, or, when the
/// record has none (it was written before reasons were coded), `text`, its
/// old string, as a [`Failure::Recorded`].
pub fn decode(why: Option<&angzarr::Diagnostic>, text: &str) -> Report {
    match why {
        Some(why) => unflatten(&why.code, &why.args),
        None => Report::of(&Failure::Recorded {
            reason: text.to_string(),
        }),
    }
}

/// The report of `code` with `args`, a name with `<name>.`-prefixed
/// siblings read as a nested cause, its value the cause's code.
fn unflatten(code: &str, args: &BTreeMap<String, String>) -> Report {
    let mut report = Report::new(code.to_string());
    for (name, value) in args
        .iter()
        .filter(|(name, _)| !name.is_empty() && !name.contains('.'))
    {
        let prefix = format!("{name}.");
        let inner: BTreeMap<String, String> = args
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_string(), v.clone())))
            .collect();
        report = match inner.is_empty() {
            true => report.arg(name.clone(), value.as_str()),
            false => report.arg(
                name.clone(),
                Arg::Nested(Box::new(unflatten(value, &inner))),
            ),
        };
    }
    report
}

#[cfg(test)]
#[path = "why.test.rs"]
mod tests;
