mod items;
mod model;
mod repository;

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

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            app.manage(ItemService::open(directory.join("yikh.sqlite3"))?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_items,
            create_item,
            update_item,
            complete_item,
            delete_item,
            item_catalog
        ])
        .run(tauri::generate_context!())
        .expect("Yikhを起動できませんでした");
}
