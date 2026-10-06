use crate::{
    assistant::{self, ChatMessage},
    assistant_tools::{AssistantTools, PendingAction},
    items::ItemService,
};
use serde::Serialize;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_action: Option<PendingAction>,
}

pub(crate) async fn send_with_tools(
    service: &ItemService,
    client: &reqwest::Client,
    id: &str,
    content: String,
    tools: &AssistantTools,
    on_changed: &(dyn Fn() + Send + Sync),
) -> Result<ConversationDetail, String> {
    tools.clear(id)?;
    let history = service.append_user_message(id, &content)?;
    let reply = assistant::answer_with_tools(
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
        assistant::ToolContext {
            runtime: tools,
            conversation_id: id,
            on_changed,
        },
    )
    .await?;
    service.append_assistant_message(id, &reply.content)?;
    let mut detail = service.get_conversation(id)?;
    detail.pending_action = reply.pending;
    Ok(detail)
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
