use crate::{
    assistant::{self, ChatMessage},
    items::ItemService,
};
use serde::Serialize;

pub(crate) async fn send(
    service: &ItemService,
    client: &reqwest::Client,
    id: &str,
    content: String,
) -> Result<ConversationDetail, String> {
    // Persist the question before network I/O without keeping a database lock.
    let history = service.append_user_message(id, &content)?;
    let answer = assistant::answer(
        service,
        client,
        content,
        history
            .into_iter()
            .map(|message| ChatMessage {
                role: message.role,
                content: message.content,
            })
            .collect(),
    )
    .await?;
    // An answer cannot recreate a conversation deleted while inference ran.
    service.append_assistant_message(id, &answer)?;
    service.get_conversation(id)
}

#[derive(Debug, Clone, Serialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

pub(crate) fn conversation_title(content: &str) -> String {
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "新しい会話".to_owned();
    }
    let mut chars = compact.chars();
    let title = chars.by_ref().take(40).collect::<String>();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}
