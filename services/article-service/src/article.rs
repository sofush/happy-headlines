use chrono::{
    DateTime,
    Utc,
};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Article {
    pub id: i64,
    pub continent: String,
    pub title: String,
    pub content: String,
    pub published_at: DateTime<Utc>,
}

impl Article {
    pub fn from_row(row: tokio_postgres::Row) -> Article {
        Article {
            id: row.get("id"),
            continent: row.get("continent"),
            title: row.get("title"),
            content: row.get("content"),
            published_at: row.get("published_at"),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct NewArticle {
    pub title: String,
    pub content: String,
}
