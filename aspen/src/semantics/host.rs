use crate::semantics::Module;
use crate::{Source, URI};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Host {
    modules: Arc<Mutex<HashMap<URI, Arc<Module>>>>,
}

impl Host {
    pub fn new() -> Host {
        Host {
            modules: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn set(&self, source: Arc<Source>) {
        let host = self.clone();
        self.modules.lock().await.insert(
            source.uri().clone(),
            Arc::new(Module::parse(source, host).await),
        );
    }

    pub async fn remove(&self, uri: &URI) {
        self.modules.lock().await.remove(uri);
    }

    pub async fn get(&self, uri: &URI) -> Option<Arc<Module>> {
        let modules = self.modules.lock().await;
        match modules.get(uri) {
            None => return None,
            Some(m) => Some(m.clone()),
        }
    }
}