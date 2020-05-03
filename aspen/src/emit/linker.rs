use crate::emit::{Emitter, OutputError, OutputResult};
use crate::semantics::Module;
use crate::Context;
use inkwell::module::Linkage;
use inkwell::AddressSpace;
use std::env::consts::ARCH;
use std::env::current_exe;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::create_dir_all;

pub struct Linker {
    context: Arc<Context>,
}

impl Linker {
    pub fn new(context: Arc<Context>) -> Linker {
        Linker { context }
    }

    async fn main_module(&self, main: &str) -> OutputResult<PathBuf> {
        let context = inkwell::context::Context::create();
        let module = context.create_module("main");
        let builder = context.create_builder();

        let i32_type = context.i32_type();
        let i8_type = context.i8_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::Generic);

        let libmain_fn = module.add_function(
            "aspen_main",
            i32_type.fn_type(&[i8_ptr_type.into()], false),
            Some(Linkage::External),
        );

        let main_fn = module.add_function("main", i32_type.fn_type(&[], false), None);
        let entry_block = context.append_basic_block(main_fn, "entry");
        builder.position_at_end(entry_block);

        let status_code = builder.build_call(
            libmain_fn,
            &[builder.build_global_string_ptr(main, "").as_pointer_value().into()],
            "",
        ).try_as_basic_value().left().unwrap();

        builder.build_return(Some(&status_code));

        let object_file_path = self.context.main_object_file_path(main);

        Emitter::write_object(&object_file_path, &module).await?;

        Ok(object_file_path)
    }

    pub async fn link(&self, modules: Vec<Arc<Module>>, main: &str) -> OutputResult<()> {
        let object_path = self.main_module(main).await?;

        let mut runtime_path = current_exe()?;
        runtime_path.pop();
        runtime_path.push("libaspen_runtime.a");

        let mut ld = tokio::process::Command::new("/usr/local/opt/llvm/bin/ld64.lld");

        ld.arg("-sdk_version")
            .arg("10.0.0")
            .arg("-arch")
            .arg(ARCH)
            .arg("-lSystem")
            .arg(runtime_path)
            .arg(object_path);

        for module in modules {
            if let Ok(object_path) = module.object_file_path() {
                ld.arg(object_path);
            }
        }

        let bin_path = self.context.binary_file_path(main);
        create_dir_all(bin_path.parent().unwrap()).await?;

        let status = ld.arg("-o").arg(&bin_path).spawn()?.await?;

        if !status.success() {
            return Err(OutputError::FailedToLink);
        }

        println!("Compiled {}", bin_path.to_str().unwrap());
        Ok(())
    }
}
