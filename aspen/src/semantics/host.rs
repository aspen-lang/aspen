use crate::semantics::module::Module;
use crate::{Source, URI};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Host {
    modules: Arc<Mutex<HashMap<URI, Module>>>,
}

impl Host {
    pub fn new() -> Host {
        Host {
            modules: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn set(&mut self, source: Arc<Source>) {
        let host = self.clone();
        self.modules
            .lock()
            .await
            .insert(source.uri().clone(), Module::parse(source, host).await);
    }

    pub async fn remove(&mut self, uri: &URI) {
        self.modules.lock().await.remove(uri);
    }

    #[cfg(test)]
    pub async fn get<F: FnOnce(Option<&Module>) -> R, R>(&self, uri: &URI, f: F) -> R {
        f(self.modules.lock().await.get(uri))
    }
}
