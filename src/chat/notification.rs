#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    markdown: String,
}

impl Notification {
    /// ```
    /// use arboters::chat::Notification;
    ///
    /// assert_eq!(Notification::new("**hello**".to_owned()).markdown(), "**hello**");
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
