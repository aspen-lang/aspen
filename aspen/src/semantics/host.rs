use crate::semantics::Module;
use crate::{Diagnostics, Source, URI};
use futures::future;
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

    pub async fn from<I: IntoIterator<Item = Arc<Source>>>(i: I) -> Self {
        let host = Host::new();
        for source in i {
            host.set(source).await;
        }
        host
    }

    pub async fn diagnostics(&self) -> Diagnostics {
        let modules: Vec<Arc<Module>> = {
            let lock = self.modules.lock().await;
            lock.values().cloned().collect::<Vec<_>>()
        };

        futures::future::join_all(
            modules
                .into_iter()
                .map(async move |m| m.diagnostics().await),
        )
        .await
        .into()
    }

    pub async fn modules(&self) -> Vec<Arc<Module>> {
        self.modules.lock().await.values().cloned().collect()
    }

    pub async fn emit(&self, main: Option<String>) {
        let modules = self.modules().await;

        let mut errors: Vec<_> = future::join_all(modules.iter().map(crate::emit::emit_module))
            .await
            .into_iter()
            .filter_map(|r| r.err())
            .collect();

        if let Some(main) = main {
            errors.extend(crate::emit::emit_main(modules, main).await.err());
        }

        if !errors.is_empty() {
            panic!(errors);
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
