use crate::generation::compile::{Compile, Print};
use crate::generation::{GenError, GenResult, ObjectFile};
use crate::semantics::Host;
use futures::future::join_all;
use inkwell::module::Linkage;
use inkwell::AddressSpace;
use std::env::consts::ARCH;
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
            let module = context.create_module("main");
            // Main module
            {
                let builder = context.create_builder();
                let main_fn =
                    module.add_function("main", context.i32_type().fn_type(&[], false), None);
                let entry_block = context.append_basic_block(main_fn, "entry");
                builder.position_at_end(entry_block);

                for m in modules {
                    let init_fn_name = format!("{:?}::init", m.uri());
                    let init_fn = module.add_function(
                        init_fn_name.as_str(),
                        context.void_type().fn_type(&[], false),
                        Some(Linkage::External),
                    );
                    builder.build_call(init_fn, &[], "");
                }

                let main_type = context.opaque_struct_type(main.as_ref());
                let main_init_fn = module.add_function(
                    main.as_ref(),
                    main_type.fn_type(&[], false),
                    Some(Linkage::External),
                );
                let main_to_string_fn = module.add_function(
                    format!("{}::ToString", main.as_ref()).as_str(),
                    context
                        .i8_type()
                        .ptr_type(AddressSpace::Generic)
                        .fn_type(&[main_type.into()], false),
                    Some(Linkage::External),
                );
                let print_fn = Print.compile(&context, &module, &builder)?;

                let main_obj = builder.build_call(main_init_fn, &[], "");
                let object_as_string = builder.build_call(
                    main_to_string_fn,
                    &[main_obj.try_as_basic_value().left().unwrap()],
                    "",
                );
                builder.build_call(
                    print_fn,
                    &[object_as_string.try_as_basic_value().left().unwrap()],
                    "",
                );

                let status_code = context.i32_type().const_int(13, false);
                builder.build_return(Some(&status_code));
            }

            host.context.ensure_object_file_dir().await?;
            objects.push(
                ObjectFile::write(host.context.main_object_file_path(main.as_ref()), module)
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
        runtime_path.push("libaspen_runtime.a");

        let mut ld = tokio::process::Command::new("/usr/local/opt/llvm/bin/ld64.lld");

        ld.arg("-sdk_version")
            .arg("10.0.0")
            .arg("-arch")
            .arg(ARCH)
            .arg("-lSystem")
            .arg(runtime_path);

        for object in objects.iter() {
            ld.arg(&object.path);
        }

        let status = ld.arg("-o").arg(&path).spawn()?.await?;

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
