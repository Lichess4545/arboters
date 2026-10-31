#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingMessage {
    markdown: String,
}

impl OutgoingMessage {
    /// ```
    /// use arboters::chat::OutgoingMessage;
    ///
    /// assert_eq!(OutgoingMessage::new("**hello**".to_owned()).markdown(), "**hello**");
    /// ```
    #[must_use]
    pub const fn new(markdown: String) -> Self {
        Self { markdown }
    }

    #[must_use]
    pub fn markdown(&self) -> &str {
        &self.markdown
    }
}
