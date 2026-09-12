//! Persona's datom text surface.
//!
//! Every value Persona reads from or writes to a command line, a request
//! file, or a report file crosses this module. The ascent — datomize,
//! protosize, print — cannot fault; the descent — delineate, conceive,
//! incorporate — can, and names its layer, path and extent when it does.

use datom_codec::{Actualizing, Budget, Composing, Datomizable, Potential};
use protos::{Protosizable, ReaderBudget, Textualizable as ProtosTextualizable};

/// The reading and composition allowance one Persona text value may spend.
///
/// Persona's requests and reports are small; the allowance is a guard against
/// a hostile or corrupt file, not a tuning knob.
pub fn budget() -> Budget {
    Budget {
        remaining: 1 << 20,
        reader: ReaderBudget { remaining: 1 << 20 },
        depth: 0,
        maximum_depth: 1024,
    }
}

/// The whole ascent into datom text. It cannot fault.
pub trait DatomTextualizable {
    fn textualize(&self) -> String;
}

impl<T> DatomTextualizable for T
where
    T: Datomizable<Output = datom_codec::Datom> + Clone,
{
    fn textualize(&self) -> String {
        self.clone().datomize(Vec::new()).protosize().textualize()
    }
}

/// The whole descent out of datom text. It may fault.
pub trait DatomActualizable: Sized {
    fn actualize_text(text: &str) -> Result<Self, datom_codec::Error>;
}

impl<T: Composing> DatomActualizable for T {
    fn actualize_text(text: &str) -> Result<Self, datom_codec::Error> {
        Potential::<Self>::from(text).actualize(&mut budget())
    }
}
