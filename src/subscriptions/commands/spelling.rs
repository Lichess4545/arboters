use strsim::damerau_levenshtein;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Closest<T> {
    One(T),
    Tie(Vec<T>),
    Nothing,
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
) -> Closest<T> {
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
    let mut nearest = nearest.into_iter();
    match (nearest.next(), nearest.len()) {
        (None, _) => Closest::Nothing,
        (Some(one), 0) => Closest::One(one),
        (Some(first), _) => Closest::Tie(std::iter::once(first).chain(nearest).collect()),
    }
}

#[cfg(test)]
mod tests;
