use std::{collections::HashMap, sync::Arc};

use lazy_static::lazy_static;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::converter::{gpu::ConverterGPU, job::Job};

pub struct AppState {
    pub jobs: HashMap<Uuid, Job>,
    pub active_processes: HashMap<Uuid, tokio::process::Child>,
    pub gpu: Option<ConverterGPU>,
    pub vaapi_device_path: Option<String>,
}

impl AppState {
    pub fn default() -> Self {
        Self {
            jobs: HashMap::new(),
            active_processes: HashMap::new(),
            gpu: None,
            vaapi_device_path: None,
        }
    }
}

lazy_static! {
    pub static ref APP_STATE: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::default()));
}
