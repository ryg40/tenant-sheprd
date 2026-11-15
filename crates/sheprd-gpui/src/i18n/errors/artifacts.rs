//! The user's `artifacts` program: `error-artifacts.ftl`.

use super::Localize;
use crate::{artifacts::Error, i18n::fl};

impl Localize for Error {
    fn localize(&self) -> String {
        // The program's own reason is untrusted: one bounded line of it.
        if let Some(reason) = self.program_reason() {
            return fl!("error-artifacts-program-said", reason = reason);
        }
        match self {
            // keep-sorted start
            Self::Address(_) => fl!("error-artifacts-address"),
            Self::Answer(_) | Self::Oversized { .. } => fl!("error-artifacts-answer"),
            Self::Busy => fl!("error-artifacts-busy"),
            Self::NoProgram => fl!("error-artifacts-no-program"),
            Self::Run(_) => fl!("error-artifacts-run"),
            // keep-sorted end
        }
    }
}
