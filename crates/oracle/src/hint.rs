use core::fmt;
use std::str::FromStr;

use kona_proof::{errors::HintParsingError, HintType};
// Add your HintWrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HintWrapper {
    Standard(HintType),
    CelestiaDA,
}

impl FromStr for HintWrapper {
    type Err = HintParsingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try parsing as standard HintType first
        if let Ok(standard) = HintType::from_str(s) {
            return Ok(HintWrapper::Standard(standard));
        }

        // Check for our custom types
        match s {
            "celestia-da" => Ok(HintWrapper::CelestiaDA),
            _ => Err(HintParsingError(String::from("unknown hint"))),
        }
    }
}

// Implement necessary traits for HintWrapper
impl fmt::Display for HintWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HintWrapper::Standard(hint) => write!(f, "{hint}"),
            HintWrapper::CelestiaDA => write!(f, "celestia-da"),
        }
    }
}
