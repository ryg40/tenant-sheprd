//! Why Servo could not draw a page, from `error-servo-page.ftl`.

use super::Localize;
use crate::{
    browser::engine::{Error, profile},
    i18n::fl,
};

impl Localize for profile::Error {
    fn localize(&self) -> String {
        match self {
            Self::InUse => fl!("error-servo-page-data-in-use"),
            Self::Io(error) => fl!("error-servo-page-data", error = error.to_string()),
            Self::NotOwn => fl!("error-servo-page-data-folder"),
            Self::Unanswered => fl!("error-servo-page-data-unanswered"),
        }
    }
}

impl Localize for Error {
    fn localize(&self) -> String {
        match self {
            Self::Address => fl!("error-servo-page-address"),
            Self::Busy => fl!("error-servo-page-busy"),
            Self::Profile(error) => error.localize(),
            Self::Stopped => fl!("error-servo-page-stopped"),
            Self::Surface(error) => fl!("error-servo-page-surface", error = error.as_str()),
            Self::Thread(error) => fl!("error-servo-page-thread", error = error.to_string()),
        }
    }
}
