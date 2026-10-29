use nutype::nutype;

#[nutype(
    sanitize(trim, lowercase),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)
)]
pub struct Name(String);

#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, Deref, Display, Deserialize)
)]
pub struct DisplayName(String);
