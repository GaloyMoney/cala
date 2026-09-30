/// Entry reads cannot reject. Posting validates entry targets before writing.
pub type EntryError = crate::CalaFault;
