#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Subscriber {
    pub email: String,
}

#[derive(serde::Deserialize)]
pub struct DeleteSubscriber {
    pub email: String,
}
