use url::Url;

use crate::chat::MessageId;

const MESSAGE_OPERATORS: [&str; 2] = ["near", "with"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkMistake {
    NotAMessageLink,
    OtherSite,
}

pub fn linked_message(text: &str, site: &Url) -> Result<MessageId, LinkMistake> {
    let text = text.trim();
    if let Some(reference) = text
        .strip_prefix("#**")
        .and_then(|rest| rest.strip_suffix("**"))
    {
        return message_reference(reference);
    }
    let link = Url::parse(text).map_err(|_| LinkMistake::NotAMessageLink)?;
    if link.origin() != site.origin() {
        return Err(LinkMistake::OtherSite);
    }
    let segments: Vec<&str> = link
        .fragment()
        .ok_or(LinkMistake::NotAMessageLink)?
        .split('/')
        .collect();
    segments
        .windows(2)
        .find_map(|pair| match pair {
            [operator, id] if MESSAGE_OPERATORS.contains(operator) => id.parse().ok(),
            _ => None,
        })
        .map(MessageId)
        .ok_or(LinkMistake::NotAMessageLink)
}

fn message_reference(reference: &str) -> Result<MessageId, LinkMistake> {
    reference
        .rsplit_once('@')
        .and_then(|(_, id)| id.parse().ok())
        .map(MessageId)
        .ok_or(LinkMistake::NotAMessageLink)
}

#[cfg(test)]
mod tests;
