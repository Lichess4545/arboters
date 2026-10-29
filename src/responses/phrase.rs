use std::borrow::Cow;

use nutype::nutype;

/// ```
/// use arboters::responses::{Matching, Phrase};
///
/// let phrase = Phrase::try_new("  Good   Game ").unwrap();
/// assert_eq!(phrase.as_str(), "Good Game");
/// assert!(phrase.appears_in("That was a GOOD game!", Matching::AnyCase));
/// assert!(!phrase.appears_in("That was a GOOD game!", Matching::ExactCase));
/// assert!(phrase.appears_in("That was a Good Game!", Matching::ExactCase));
/// assert!(!phrase.appears_in("goodgame", Matching::AnyCase));
/// ```
#[nutype(
    sanitize(with = |phrase: String| phrase.split_whitespace().collect::<Vec<_>>().join(" ")),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)
)]
pub struct Phrase(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Matching {
    AnyCase,
    ExactCase,
}

impl Matching {
    fn compared(self, text: &str) -> Cow<'_, str> {
        match self {
            Self::AnyCase => Cow::Owned(text.to_lowercase()),
            Self::ExactCase => Cow::Borrowed(text),
        }
    }
}

impl Phrase {
    #[must_use]
    pub fn appears_in(&self, text: &str, matching: Matching) -> bool {
        let text: Vec<char> = matching.compared(text).chars().collect();
        let words: Vec<Vec<char>> = self
            .split(' ')
            .map(|word| matching.compared(word).chars().collect())
            .collect();
        (0..text.len()).any(|start| starts_at(&text, start, &words))
    }

    #[must_use]
    pub fn ignoring_case(&self) -> String {
        self.to_lowercase()
    }
}

fn starts_at(text: &[char], start: usize, words: &[Vec<char>]) -> bool {
    let before = start.checked_sub(1).and_then(|index| text.get(index));
    if joined(before, words.first().and_then(|word| word.first())) {
        return false;
    }
    let mut at = start;
    for (position, word) in words.iter().enumerate() {
        if position > 0 {
            let gap = text.get(at..).map_or(0, |rest| {
                rest.iter().take_while(|c| c.is_whitespace()).count()
            });
            if gap == 0 {
                return false;
            }
            at += gap;
        }
        if text.get(at..at + word.len()) != Some(word.as_slice()) {
            return false;
        }
        at += word.len();
    }
    !joined(words.last().and_then(|word| word.last()), text.get(at))
}

fn joined(left: Option<&char>, right: Option<&char>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left.is_alphanumeric() && right.is_alphanumeric())
}

#[cfg(test)]
mod tests;
