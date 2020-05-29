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

pub struct ExecutableBuilder {
    pub host: Host,
    pub main: Option<String>,
    pub static_linkage: bool,
}

impl ExecutableBuilder {
    pub fn new(host: Host) -> ExecutableBuilder {
        ExecutableBuilder {
            host,
            main: None,
            static_linkage: false,
        }
    }

    pub fn main<M: Into<String>>(&mut self, main: M) -> &mut Self {
        self.main = Some(main.into());
        self
    }

    pub fn link_statically(&mut self) -> &mut Self {
        self.static_linkage = true;
        self
    }

    pub async fn write(&self) -> GenResult<Executable> {
        Executable::new(self).await
    }
}

impl Executable {
    pub fn build(host: Host) -> ExecutableBuilder {
        ExecutableBuilder::new(host)
    }

    async fn new(builder: &ExecutableBuilder) -> GenResult<Executable> {
        let host = &builder.host;
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
            return Err(GenError::Multi(errors));
        }

        if let Some(main) = builder.main.as_ref() {
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

            Executable::link_executable(path, objects, builder.static_linkage).await
        } else {
            host.context.ensure_binary_dir().await?;
            if builder.static_linkage {
                let path = host.context.binary_archive_file_path()?;
                Executable::link_archive(path, objects).await
            } else {
                let path = host.context.binary_dylib_file_path()?;
                Executable::link_dylib(path, objects).await
            }
        }
    }

    async fn link_executable(
        path: PathBuf,
        objects: Vec<ObjectFile>,
        statically: bool,
    ) -> GenResult<Executable> {
        let mut runtime_path = current_exe()?;
        runtime_path.pop();

        let mut cc = std::process::Command::new("cc");
        if statically {
            cc.arg("-static");
        }

        for object in objects.iter() {
            cc.arg(&object.path);
        }

        cc.arg(format!("-L{}", runtime_path.display()))
            .arg("-laspen_runtime");

        if cfg!(target_os = "linux") {
            cc.arg("-no-pie");
            cc.arg("-lpthread");
            cc.arg("-lm");

            if !statically {
                cc.arg("-ldl");
            }
        }

        cc.arg("-o").arg(&path);

        let command = format!("{:?}", cc);

        let status = tokio::process::Command::from(cc).spawn()?.await?;

        if !status.success() {
            return Err(GenError::FailedToLink(command));
        }

        if statically {
            let strip = std::process::Command::new("strip")
                .arg(&path);
            let command = format!("{:?}", strip);
            let status = tokio::process::Command::from(strip).spawn()?.await?;
            if !status.success() {
                eprintln!("Failed to strip static executable");
            }
        }

        Ok(Executable { objects, path })
    }

    async fn link_archive(path: PathBuf, objects: Vec<ObjectFile>) -> GenResult<Executable> {
        let mut runtime_path = current_exe()?;
        runtime_path.pop();

        let mut cc = std::process::Command::new("cc");

        for object in objects.iter() {
            cc.arg(&object.path);
        }

        cc.arg(format!("-L{}", runtime_path.display()))
            .arg("-laspen_runtime");

        if cfg!(target_os = "linux") {
            cc.arg("-no-pie");
            cc.arg("-lpthread");
            cc.arg("-lm");
            cc.arg("-ldl");
        }

        cc.arg("-o").arg(&path);

        let command = format!("{:?}", cc);

        let status = tokio::process::Command::from(cc).spawn()?.await?;

        if !status.success() {
            return Err(GenError::FailedToLink(command));
        }

        Ok(Executable { objects, path })
    }

    async fn link_dylib(path: PathBuf, objects: Vec<ObjectFile>) -> GenResult<Executable> {
        let mut runtime_path = current_exe()?;
        runtime_path.pop();

        let mut cc = std::process::Command::new("cc");

        for object in objects.iter() {
            cc.arg(&object.path);
        }

        cc.arg(format!("-L{}", runtime_path.display()))
            .arg("-laspen_runtime");

        if cfg!(target_os = "linux") {
            cc.arg("-no-pie");
            cc.arg("-lpthread");
            cc.arg("-lm");
            cc.arg("-ldl");
        }

        cc.arg("-o").arg(&path);

        let command = format!("{:?}", cc);

        let status = tokio::process::Command::from(cc).spawn()?.await?;

        if !status.success() {
            return Err(GenError::FailedToLink(command));
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
