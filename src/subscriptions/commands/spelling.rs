use strsim::damerau_levenshtein;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found<T> {
    One(T),
    Several(Vec<T>),
    Nothing,
}

impl<T> From<Vec<T>> for Found<T> {
    fn from(mut found: Vec<T>) -> Self {
        match found.len() {
            0 | 1 => found.pop().map_or(Self::Nothing, Self::One),
            _ => Self::Several(found),
        }
    }
}

fn allowed_edits(word: &str) -> usize {
    match word.chars().count() {
        0..=2 => 0,
        3..=5 => 1,
        _ => 2,
    }
}

pub fn matches(written: &str, word: &str) -> bool {
    damerau_levenshtein(written, word) <= allowed_edits(word)
}

pub fn closest<'a, T>(
    written: &str,
    candidates: impl IntoIterator<Item = (&'a str, T)>,
) -> Found<T> {
    let mut best: Option<usize> = None;
    let mut nearest = Vec::new();
    for (word, candidate) in candidates {
        let distance = damerau_levenshtein(written, word);
        if distance > allowed_edits(word) || best.is_some_and(|best| distance > best) {
            continue;
        }
        if best.is_none_or(|best| distance < best) {
            best = Some(distance);
            nearest.clear();
        }
        nearest.push(candidate);
    }
    nearest.into()
}

#[cfg(test)]
mod tests;
