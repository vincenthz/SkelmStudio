//! Turning a chat into the transcript given to the model, and the reasoning
//! ("thinking") blocks of the model's replies.

use skelm_exec::Message;

use crate::store::{Chat, ChatMessage, Project, Role};

/// The system prompt of a chat: the project's instructions and reference
/// files, then the chat's own system prompt
pub fn system_prompt(chat: &Chat, project: Option<&Project>) -> String {
    let mut parts = Vec::new();
    if let Some(project) = project {
        if !project.instructions.trim().is_empty() {
            parts.push(project.instructions.trim().to_string());
        }
        if !project.files.is_empty() {
            let mut files = String::from(
                "The following reference files are part of this project. \
                 Use them to answer when relevant.",
            );
            for file in &project.files {
                files.push_str(&format!(
                    "\n\n<file name=\"{}\">\n{}\n</file>",
                    file.name,
                    file.content.trim_end()
                ));
            }
            parts.push(files);
        }
    }
    if !chat.system_prompt.trim().is_empty() {
        parts.push(chat.system_prompt.trim().to_string());
    }
    parts.join("\n\n")
}

/// The text of a user message given to the model, with its attachments
pub fn user_content(message: &ChatMessage) -> String {
    let mut content = message.content.clone();
    for attachment in &message.attachments {
        content.push_str(&format!(
            "\n\n<attachment name=\"{}\">\n{}\n</attachment>",
            attachment.name,
            attachment.content.trim_end()
        ));
    }
    content
}

/// The transcript of `messages` (a prefix of the chat's) for the model
pub fn transcript(chat: &Chat, project: Option<&Project>, messages: &[ChatMessage]) -> Vec<Message> {
    let mut out = Vec::new();
    let system = system_prompt(chat, project);
    if !system.is_empty() {
        out.push(Message::system(system));
    }
    for message in messages {
        match message.role {
            Role::System => out.push(Message::system(message.content.clone())),
            Role::User => out.push(Message::user(user_content(message))),
            Role::Assistant => {
                // the reasoning isn't fed back, as the templates expect
                let answer = split_reasoning(&message.content).answer.trim().to_string();
                if answer.is_empty() && message.error.is_some() {
                    continue;
                }
                out.push(Message::assistant(answer));
            }
        }
    }
    out
}

/// A reply split into its reasoning and its answer
#[derive(Debug, PartialEq, Eq)]
pub struct Reply<'a> {
    pub reasoning: Option<&'a str>,
    /// the reasoning is complete (its closing tag was seen)
    pub reasoning_done: bool,
    pub answer: &'a str,
}

const OPEN_TAGS: &[&str] = &["<think>", "<thinking>", "<reasoning>"];
const CLOSE_TAGS: &[&str] = &["</think>", "</thinking>", "</reasoning>"];

/// Split a reply into reasoning and answer: `<think>…</think>answer`, or
/// `…</think>answer` when the chat template opened the block in the prompt.
pub fn split_reasoning(text: &str) -> Reply<'_> {
    let trimmed = text.trim_start();
    for (open, close) in OPEN_TAGS.iter().zip(CLOSE_TAGS) {
        if let Some(rest) = trimmed.strip_prefix(open) {
            return match rest.find(close) {
                Some(end) => Reply {
                    reasoning: Some(rest[..end].trim()),
                    reasoning_done: true,
                    answer: rest[end + close.len()..].trim_start(),
                },
                None => Reply {
                    reasoning: Some(rest.trim_start()),
                    reasoning_done: false,
                    answer: "",
                },
            };
        }
    }
    for close in CLOSE_TAGS {
        if let Some(end) = text.find(close) {
            return Reply {
                reasoning: Some(text[..end].trim()),
                reasoning_done: true,
                answer: text[end + close.len()..].trim_start(),
            };
        }
    }
    Reply {
        reasoning: None,
        reasoning_done: true,
        answer: text,
    }
}

/// A title for a chat, from its first message
pub fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("New Chat");
    let line = line.trim_start_matches('#').trim();
    const MAX: usize = 48;
    if line.chars().count() <= MAX {
        line.to_string()
    } else {
        let cut: String = line.chars().take(MAX).collect();
        let cut = match cut.rfind(' ') {
            Some(i) if i > MAX / 2 => &cut[..i],
            _ => &cut,
        };
        format!("{}…", cut.trim_end())
    }
}

/// The chat as a Markdown document
pub fn to_markdown(chat: &Chat) -> String {
    let mut out = format!("# {}\n\n", chat.title);
    if !chat.system_prompt.trim().is_empty() {
        out.push_str(&format!("> **System:** {}\n\n", chat.system_prompt.trim()));
    }
    for message in &chat.messages {
        match message.role {
            Role::System => out.push_str(&format!("**System**\n\n{}\n\n", message.content)),
            Role::User => {
                out.push_str(&format!("## You\n\n{}\n\n", message.content.trim()));
                for a in &message.attachments {
                    out.push_str(&format!("*Attached: {}*\n\n", a.name));
                }
            }
            Role::Assistant => {
                let reply = split_reasoning(&message.content);
                let who = message.model.as_deref().unwrap_or("Assistant");
                out.push_str(&format!("## {who}\n\n"));
                if let Some(reasoning) = reply.reasoning {
                    for line in reasoning.lines() {
                        out.push_str(&format!("> {line}\n"));
                    }
                    out.push('\n');
                }
                out.push_str(reply.answer.trim());
                out.push_str("\n\n");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_split() {
        assert_eq!(
            split_reasoning("<think>hmm</think>\n\nHello"),
            Reply {
                reasoning: Some("hmm"),
                reasoning_done: true,
                answer: "Hello"
            }
        );
        assert_eq!(
            split_reasoning("<think>still going"),
            Reply {
                reasoning: Some("still going"),
                reasoning_done: false,
                answer: ""
            }
        );
        assert_eq!(
            split_reasoning("opened in prompt</think>Answer"),
            Reply {
                reasoning: Some("opened in prompt"),
                reasoning_done: true,
                answer: "Answer"
            }
        );
        assert_eq!(split_reasoning("plain").reasoning, None);
    }

    #[test]
    fn titles() {
        assert_eq!(title_from("\n  # Hello there\nmore"), "Hello there");
        let long = "word ".repeat(30);
        let t = title_from(&long);
        assert!(t.ends_with('…') && t.chars().count() <= 49);
    }
}
