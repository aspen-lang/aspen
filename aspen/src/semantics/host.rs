use crate::semantics::Module;
use crate::{Context, Diagnostics, Range, Source, URI};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Host {
    pub context: Arc<Context>,
    modules: Arc<Mutex<HashMap<URI, Arc<Module>>>>,
}

impl Host {
    pub fn new(context: Arc<Context>) -> Host {
        Host {
            context,
            modules: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn from<I: IntoIterator<Item = Arc<Source>>>(context: Arc<Context>, i: I) -> Self {
        let host = Host::new(context);
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

    pub async fn set(&self, source: Arc<Source>) -> Arc<Module> {
        let host = self.clone();
        let mut modules = self.modules.lock().await;
        let uri = source.uri().clone();
        modules.insert(uri.clone(), Arc::new(Module::parse(source, host).await));
        modules.get(&uri).unwrap().clone()
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

    pub async fn apply_edits<I: IntoIterator<Item = (Option<Range>, String)>>(
        &self,
        uri: &URI,
        edits: I,
    ) {
        if let Some(module) = self.get(uri).await {
            let new = module.source.apply_edits(edits);

            self.set(new).await;
        }
    }
}
