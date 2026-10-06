use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind { Task, Bute }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus { Active, Completed }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority { None, Low, Medium, High }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    pub title: String,
    pub notes: String,
    pub status: ItemStatus,
    pub project: Option<String>,
    pub scheduled_date: Option<String>,
    pub due_date: Option<String>,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemInput {
    pub kind: ItemKind,
    pub title: String,
    pub notes: String,
    pub project: Option<String>,
    pub scheduled_date: Option<String>,
    pub due_date: Option<String>,
    pub priority: Priority,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemQuery {
    pub kind: Option<ItemKind>,
    pub status: Option<ItemStatus>,
    pub project: Option<String>,
    pub search: Option<String>,
    pub due_from: Option<String>,
    pub due_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub projects: Vec<String>,
    pub tags: Vec<String>,
}
