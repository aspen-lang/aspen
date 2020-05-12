use crate::generation::{GenError, GenResult, Generator, ObjectFile};
use crate::semantics::Host;
use futures::future::join_all;
use std::env::{current_dir, current_exe};
use std::fmt;
use std::path::PathBuf;

pub struct Executable {
    pub path: PathBuf,
    pub objects: Vec<ObjectFile>,
}

impl Executable {
    pub async fn new<M: AsRef<str>>(host: Host, main: M) -> GenResult<Executable> {
        let modules = host.modules().await;
        let object_results =
            join_all(modules.iter().map(|module| ObjectFile::new(module.clone()))).await;

        let mut objects = vec![];
        let mut errors = vec![];

        for object_result in object_results {
            match object_result {
                Ok(o) => objects.push(o),
                Err(e) => errors.push(e),
            }
        }

        if errors.len() > 0 {
            Err(GenError::Multi(errors))
        } else {
            let context = inkwell::context::Context::create();
            let generator = Generator::new(host.clone(), &context);

            let emitted_module = generator.generate_main(main.as_ref())?;

            host.context.ensure_object_file_dir().await?;
            objects.push(
                ObjectFile::write(
                    host.context.main_object_file_path(main.as_ref()),
                    emitted_module,
                )
                .await?,
            );

            let path = host.context.binary_file_path(main.as_ref());
            host.context.ensure_binary_dir().await?;
            Executable::write(path, objects).await
        }
    }

    pub(crate) async fn write(path: PathBuf, objects: Vec<ObjectFile>) -> GenResult<Executable> {
        let mut runtime_path = current_exe()?;
        runtime_path.pop();

        let mut ld = tokio::process::Command::new("cc");

        ld.arg("-static");

        for object in objects.iter() {
            ld.arg(&object.path);
        }

        ld.arg(format!("-L{}", runtime_path.display())).arg("-laspen_runtime");

        if cfg!(target_os = "macos") {
        }

        if cfg!(target_os = "linux") {
        }

        ld.arg("-o").arg(&path);

        let status = ld.spawn()?.await?;

        if !status.success() {
            return Err(GenError::FailedToLink);
        }

        Ok(Executable { objects, path })
    }
}

impl fmt::Display for Executable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            current_dir()
                .ok()
                .and_then(|cwd| self.path.strip_prefix(cwd).ok())
                .unwrap_or(&self.path)
                .display()
        )
    }
}
