use std::{collections::HashMap, sync::Arc};

use lazy_static::lazy_static;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::converter::{gpu::ConverterGPU, job::Job};

pub struct AppState {
    pub jobs: HashMap<Uuid, Job>,
    pub active_processes: HashMap<Uuid, tokio::process::Child>,
    pub gpu: Option<ConverterGPU>,
}

impl AppState {
    pub fn default() -> Self {
        Self {
            jobs: HashMap::new(),
            active_processes: HashMap::new(),
            gpu: None,
        }
    }
}

lazy_static! {
    pub static ref APP_STATE: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::default()));
}
