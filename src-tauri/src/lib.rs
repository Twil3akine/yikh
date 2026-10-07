mod assistant;
mod assistant_policy;
mod assistant_tools;
mod conversations;
mod items;
mod model;
mod repository;

use assistant::{AssistantSettings, ChatMessage};
use assistant_tools::AssistantTools;
use conversations::{Conversation, ConversationDetail};
use items::ItemService;
use model::{Catalog, Item, ItemInput, ItemQuery};
use tauri::{Emitter, Manager, State};

#[tauri::command]
fn list_items(service: State<'_, ItemService>, query: ItemQuery) -> Result<Vec<Item>, String> {
    service.query(&query)
}

#[tauri::command]
fn create_item(service: State<'_, ItemService>, input: ItemInput) -> Result<Item, String> {
    service.create(input)
}

#[tauri::command]
fn update_item(
    service: State<'_, ItemService>,
    id: String,
    input: ItemInput,
) -> Result<Item, String> {
    service.update(&id, input)
}

#[tauri::command]
fn complete_item(service: State<'_, ItemService>, id: String) -> Result<Item, String> {
    service.complete(&id)
}

#[tauri::command]
fn delete_item(service: State<'_, ItemService>, id: String) -> Result<(), String> {
    service.delete(&id)
}

#[tauri::command]
fn item_catalog(service: State<'_, ItemService>) -> Result<Catalog, String> {
    service.catalog()
}

struct AssistantClient(Result<reqwest::Client, String>);

#[tauri::command]
fn open_assistant_link(url: String) -> Result<(), String> {
    let url = reqwest::Url::parse(&url).map_err(|_| "リンクを開けませんでした。".to_owned())?;
    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        return Err("この形式のリンクは開けません。".to_owned());
    }
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url.as_str())
            .status()
            .map_err(|_| "リンクを開けませんでした。".to_owned())?;
        if status.success() {
            Ok(())
        } else {
            Err("リンクを開けませんでした。".to_owned())
        }
    }
    #[cfg(not(target_os = "macos"))]
    Err("この環境ではリンクを開けません。".to_owned())
}

#[tauri::command]
fn get_assistant_settings(service: State<'_, ItemService>) -> Result<AssistantSettings, String> {
    assistant::get_settings(&service)
}

#[tauri::command]
fn save_assistant_settings(
    service: State<'_, ItemService>,
    settings: AssistantSettings,
) -> Result<AssistantSettings, String> {
    assistant::save_settings(&service, settings)
}

#[tauri::command]
async fn ask_assistant(
    service: State<'_, ItemService>,
    client: State<'_, AssistantClient>,
    message: String,
    history: Vec<ChatMessage>,
) -> Result<String, String> {
    let client = client.inner().0.as_ref().map_err(Clone::clone)?;
    assistant::answer(&service, client, message, history).await
}

#[tauri::command]
fn list_conversations(service: State<'_, ItemService>) -> Result<Vec<Conversation>, String> {
    service.list_conversations()
}

#[tauri::command]
fn create_conversation(service: State<'_, ItemService>) -> Result<ConversationDetail, String> {
    service.create_conversation()
}

#[tauri::command]
fn get_conversation(
    service: State<'_, ItemService>,
    tools: State<'_, AssistantTools>,
    id: String,
) -> Result<ConversationDetail, String> {
    let mut detail = service.get_conversation(&id)?;
    detail.pending_action = tools.pending(&id)?;
    Ok(detail)
}

#[tauri::command]
fn delete_conversation(
    service: State<'_, ItemService>,
    tools: State<'_, AssistantTools>,
    id: String,
) -> Result<(), String> {
    tools.clear(&id)?;
    service.delete_conversation(&id)
}

#[tauri::command]
async fn send_conversation_message(
    service: State<'_, ItemService>,
    client: State<'_, AssistantClient>,
    tools: State<'_, AssistantTools>,
    app: tauri::AppHandle,
    id: String,
    content: String,
) -> Result<ConversationDetail, String> {
    let client = client.inner().0.as_ref().map_err(Clone::clone)?;
    conversations::send_with_tools(&service, client, &id, content, &tools, &|| {
        let _ = app.emit("items-changed", ());
    })
    .await
}

#[tauri::command]
fn resolve_assistant_action(
    service: State<'_, ItemService>,
    tools: State<'_, AssistantTools>,
    app: tauri::AppHandle,
    input: AssistantActionInput,
) -> Result<ConversationDetail, String> {
    service.get_conversation(&input.id)?;
    let result = tools.resolve(
        &service,
        &input.id,
        &input.token,
        input.candidate_key.as_deref(),
        input.confirm,
    )?;
    if result.changed {
        let _ = app.emit("items-changed", ());
    }
    let message = result
        .output
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("操作を確認してください。");
    service.append_assistant_message(&input.id, message)?;
    let mut detail = service.get_conversation(&input.id)?;
    detail.pending_action = result.pending;
    Ok(detail)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssistantActionInput {
    id: String,
    token: String,
    candidate_key: Option<String>,
    confirm: bool,
}

#[tauri::command]
fn cancel_assistant_action(
    service: State<'_, ItemService>,
    tools: State<'_, AssistantTools>,
    id: String,
    token: String,
) -> Result<ConversationDetail, String> {
    service.get_conversation(&id)?;
    tools.cancel(&id, &token)?;
    service.append_assistant_message(&id, "操作をキャンセルしました。")?;
    service.get_conversation(&id)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            app.manage(ItemService::open(directory.join("yikh.sqlite3"))?);
            app.manage(AssistantClient(assistant::http_client()));
            app.manage(AssistantTools::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_items,
            create_item,
            update_item,
            complete_item,
            delete_item,
            item_catalog,
            get_assistant_settings,
            save_assistant_settings,
            ask_assistant,
            open_assistant_link,
            list_conversations,
            create_conversation,
            get_conversation,
            delete_conversation,
            send_conversation_message,
            resolve_assistant_action,
            cancel_assistant_action
        ])
        .run(tauri::generate_context!())
        .expect("Yikhを起動できませんでした");
}
