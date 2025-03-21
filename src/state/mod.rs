use crate::job::Job;
use lazy_static::lazy_static;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct AppState {
    pub jobs: HashMap<Uuid, Job>,
}

impl AppState {
    pub fn default() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }
}

lazy_static! {
    pub static ref APP_STATE: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::default()));
}
