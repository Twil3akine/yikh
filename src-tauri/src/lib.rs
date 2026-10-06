mod assistant;
mod items;
mod model;
mod repository;

use assistant::{AssistantSettings, ChatMessage};
use items::ItemService;
use model::{Catalog, Item, ItemInput, ItemQuery};
use tauri::{Manager, State};

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

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            app.manage(ItemService::open(directory.join("yikh.sqlite3"))?);
            app.manage(AssistantClient(assistant::http_client()));
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
            ask_assistant
        ])
        .run(tauri::generate_context!())
        .expect("Yikhを起動できませんでした");
}
