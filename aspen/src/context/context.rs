use std::env::current_dir;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{self, DirEntry};
use std::fs::Metadata;
use std::fmt;

/// The context in which the compiler will work with code
/// emission and configuration.
///
/// It is established by traversing the file system upward
/// starting from the current working directory.
pub struct Context {
    parent: Option<Arc<Context>>,
    kind: ContextKind,
}

#[derive(Debug, Clone)]
enum ContextKind {
    Global,
    Directory(PathBuf),
    Temporary,
}

impl Context {
    pub async fn infer() -> io::Result<Arc<Context>> {
        let mut local = Self::from(current_dir()?).await?;
        if local.is_global() {
            local = Arc::new(Context {
                parent: Some(local),
                kind: ContextKind::Temporary,
            });
        }
        Ok(local)
    }

    async fn from(dir: PathBuf) -> io::Result<Arc<Context>> {
        let mut context_stack = vec![Self::find(dir).await?];
        while let Some(ContextKind::Directory(dir)) =
            &context_stack.last().map(|c| &c.kind).cloned()
        {
            if let Some(parent_dir) = dir.parent() {
                context_stack.push(Self::find(parent_dir.to_path_buf()).await?);
            } else {
                context_stack.push(Context {
                    parent: None,
                    kind: ContextKind::Global,
                });
            }
        }
        Ok(context_stack
            .into_iter()
            .rfold(None, |parent, mut context| {
                context.parent = parent;
                Some(Arc::new(context))
            })
            .unwrap())
    }

    async fn find(mut dir: PathBuf) -> io::Result<Context> {
        loop {
            let mut entries = fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                if Self::is_context_boundary_marker(entry)
                    .await
                    .unwrap_or(false)
                {
                    return Ok(Context {
                        parent: None,
                        kind: ContextKind::Directory(dir),
                    });
                }
            }
            match dir.parent() {
                None => {
                    return Ok(Context {
                        parent: None,
                        kind: ContextKind::Global,
                    })
                }

                Some(parent) => dir = parent.to_path_buf(),
            }
        }
    }

    async fn is_context_boundary_marker(entry: DirEntry) -> io::Result<bool> {
        let metadata = entry.metadata().await?;
        Ok(Self::is_git_root(&entry, &metadata) || Self::is_mod_root(&entry, &metadata) || Self::is_pkg_root(&entry, &metadata))
    }

    fn is_git_root(entry: &DirEntry, metadata: &Metadata) -> bool {
        metadata.is_dir() && entry.file_name() == ".git"
    }

    fn is_mod_root(entry: &DirEntry, metadata: &Metadata) -> bool {
        metadata.is_file() && entry.file_name() == "mod.yml"
    }

    fn is_pkg_root(entry: &DirEntry, metadata: &Metadata) -> bool {
        metadata.is_file() && entry.file_name() == "pkg.yml"
    }

    fn is_global(&self) -> bool {
        if let ContextKind::Global = self.kind {
            true
        } else {
            false
        }
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.kind)?;
        if let Some(parent) = &self.parent {
            if parent.parent.is_some() {
                write!(f, "\n├ {:?}", parent)
            } else {
                write!(f, "\n└ {:?}", parent)
            }
        } else {
            Ok(())
        }
    }
}
