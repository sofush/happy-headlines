#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Comment {
    pub id: i64,
    pub continent: String,
    pub article_id: i64,
    pub author: String,
    pub content: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct NewComment {
    pub continent: String,
    pub article_id: i64,
    pub author: String,
    pub content: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct MaskComment {
    pub author: String,
    pub content: String,
}

#[derive(serde::Deserialize)]
pub struct CommentQuery {
    pub continent: String,
    pub article_id: i64,
}
