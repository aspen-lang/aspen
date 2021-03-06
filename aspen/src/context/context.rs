use crate::semantics::Host;
use crate::URI;
use mktemp::Temp;
use std::convert::TryInto;
use std::env::consts::{DLL_EXTENSION, DLL_PREFIX, EXE_EXTENSION};
use std::env::current_dir;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::Metadata;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{self, DirEntry};
use tokio::io::AsyncWriteExt;

/// The context in which the compiler will work with code
/// emission and configuration.
///
/// It is established by traversing the file system upward
/// starting from the current working directory.
pub struct Context {
    parent: Option<Arc<Context>>,
    kind: ContextKind,
}

#[derive(Clone)]
enum ContextKind {
    Global(PathBuf),
    Directory(PathBuf),
    Temporary(Temp),

    #[cfg(test)]
    Test,
}

impl Context {
    pub fn temporary(parent: Option<Arc<Context>>) -> io::Result<Context> {
        Ok(Self::new(parent, ContextKind::Temporary(Temp::new_dir()?)))
    }

    #[cfg(test)]
    pub fn test() -> Context {
        Context {
            parent: None,
            kind: ContextKind::Test,
        }
    }

    pub fn directory(parent: Option<Arc<Context>>, dir: PathBuf) -> Context {
        Self::new(parent, ContextKind::Directory(dir))
    }

    pub fn global() -> io::Result<Context> {
        let mut dir = current_dir()?;
        while let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        }
        Ok(Self::new(None, ContextKind::Global(dir)))
    }

    fn new(parent: Option<Arc<Context>>, kind: ContextKind) -> Context {
        Context { parent, kind }
    }

    pub async fn infer() -> io::Result<Arc<Context>> {
        Self::infer_from(current_dir()?).await
    }

    pub async fn infer_from(root: PathBuf) -> io::Result<Arc<Context>> {
        let mut local = Self::from(root).await?;
        if local.is_global() {
            local = Arc::new(Self::temporary(Some(local))?);
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
                context_stack.push(Self::global()?);
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
                    return Ok(Self::directory(None, dir));
                }
            }
            match dir.parent() {
                None => {
                    return Self::global();
                }

                Some(parent) => dir = parent.to_path_buf(),
            }
        }
    }

    async fn is_context_boundary_marker(entry: DirEntry) -> io::Result<bool> {
        let metadata = entry.metadata().await?;
        Ok(Self::is_git_root(&entry, &metadata)
            || Self::is_mod_root(&entry, &metadata)
            || Self::is_pkg_root(&entry, &metadata))
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
        if let ContextKind::Global(_) = self.kind {
            true
        } else {
            false
        }
    }

    pub fn root_dir(&self) -> io::Result<PathBuf> {
        match &self.kind {
            ContextKind::Temporary(_) => current_dir(),
            ContextKind::Directory(dir) => dir.canonicalize(),
            ContextKind::Global(dir) => Ok(dir.clone()),

            #[cfg(test)]
            ContextKind::Test => Err(io::ErrorKind::PermissionDenied.into()),
        }
    }

    fn workspace_dir(&self, subdir: Option<&str>) -> PathBuf {
        let mut dir = match &self.kind {
            ContextKind::Temporary(tmp) => tmp.to_path_buf(),
            ContextKind::Directory(dir) => {
                let mut dir = dir.clone();
                dir.push(".aspen");
                dir
            }
            ContextKind::Global(dir) => {
                let mut dir = dirs::home_dir().unwrap_or(dir.clone());
                dir.push(".aspen");
                dir
            }

            #[cfg(test)]
            ContextKind::Test => {
                let mut dir = current_dir().unwrap();
                dir.push(".test-aspen");
                dir
            }
        };
        if let Some(s) = subdir {
            dir.push(s);
        }
        dir
    }

    fn in_workspace(&self, subdir: Option<&str>, path: PathBuf) -> io::Result<PathBuf> {
        let root = self.root_dir()?;
        if !path.starts_with(&root) {
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let relative = path.strip_prefix(&root).unwrap();
        Ok(self.workspace_dir(subdir).join(relative))
    }

    pub fn object_file_path(&self, uri: &URI) -> io::Result<PathBuf> {
        let mut path: PathBuf = uri.try_into()?;
        path.set_extension("o");
        self.in_workspace(Some("cache"), path)
    }

    pub fn header_file_path(&self, uri: &URI) -> io::Result<PathBuf> {
        let mut path: PathBuf = uri.try_into()?;
        path.set_extension("ah");
        self.in_workspace(Some("cache"), path)
    }

    pub fn main_object_file_path(&self, main: &str) -> PathBuf {
        let mut path = self.workspace_dir(Some("cache"));
        path.push(main);
        path.set_extension("main.o");
        path
    }

    pub fn binary_file_path(&self, main: &str) -> PathBuf {
        let mut path = self.out_dir();
        path.push(main);
        path.set_extension(EXE_EXTENSION);
        path
    }

    fn out_dir(&self) -> PathBuf {
        match &self.kind {
            ContextKind::Temporary(tmp) => self.root_dir().unwrap_or(tmp.to_path_buf()),
            _ => self.workspace_dir(Some("out")),
        }
    }

    pub fn binary_archive_file_path(&self) -> io::Result<PathBuf> {
        let current_dir = current_dir()?;
        let mut name = OsString::new();
        name.push(DLL_PREFIX);
        name.push(current_dir.file_name().unwrap());
        let mut path = self.out_dir();
        path.push(name);
        if cfg!(windows) {
            path.set_extension("lib");
        } else {
            path.set_extension("a");
        }
        Ok(path)
    }

    pub fn binary_dylib_file_path(&self) -> io::Result<PathBuf> {
        let current_dir = current_dir()?;
        let mut name = OsString::new();
        name.push(DLL_PREFIX);
        name.push(current_dir.file_name().unwrap());
        let mut path = self.out_dir();
        path.push(name);
        path.set_extension(DLL_EXTENSION);
        Ok(path)
    }

    pub fn host(self: &Arc<Self>) -> Host {
        Host::new(self.clone())
    }

    pub fn name(&self) -> Option<String> {
        self.root_dir().ok().and_then(|d| {
            d.file_name()
                .and_then(OsStr::to_str)
                .map(ToString::to_string)
        })
    }

    pub async fn ensure_workspace_dir(&self, subdir: Option<&str>) -> io::Result<()> {
        let dir = self.workspace_dir(subdir);
        tokio::fs::create_dir_all(&dir).await?;

        let gitignore_path = self.workspace_dir(Some(".gitignore"));
        if !tokio::fs::metadata(&gitignore_path).await.is_ok() {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(&gitignore_path)
                .await?;
            file.write_all("cache/\nout/\n".as_bytes()).await?;
        }

        Ok(())
    }

    pub async fn ensure_binary_dir(&self) -> io::Result<()> {
        self.ensure_workspace_dir(Some("out")).await
    }

    pub async fn ensure_object_file_dir(&self) -> io::Result<()> {
        self.ensure_workspace_dir(Some("cache")).await
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

impl fmt::Debug for ContextKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ContextKind::Global(p) => write!(f, "Global {:?}", p),
            ContextKind::Directory(p) => write!(f, "Directory {:?}", p),
            ContextKind::Temporary(p) => write!(f, "Temporary {:?}", p.as_os_str()),

            #[cfg(test)]
            ContextKind::Test => write!(f, "Test"),
        }
    }
}
