use crate::semantics::Module as AModule;
use inkwell::context::Context;
use inkwell::module::Linkage;
use std::io::{Write, Result, ErrorKind};
use std::process::Stdio;
use std::sync::Arc;
use inkwell::AddressSpace;
use std::env::consts::ARCH;
use std::env::current_exe;

pub async fn emit_module(aspen_module: &Arc<AModule>) -> Result<()> {
    let object_path = match aspen_module.object_file_path() {
        None => return Err(ErrorKind::InvalidInput.into()),
        Some(p) => p,
    };

    let context = Context::create();
    let module = context.create_module(aspen_module.uri().as_ref());

    let void_type = context.void_type();

    for (name, _) in aspen_module.exported_declarations().await {
        {
            let fn_value = module.add_function(name.as_str(), void_type.fn_type(&[], false), None);
            let entry_block = context.append_basic_block(fn_value, "entry");

            let builder = context.create_builder();
            builder.position_at_end(entry_block);

            builder.build_return(None);
        }

        if name == "Main" {
            let i8_type = context.i8_type();
            let i8_pointer_type = i8_type.ptr_type(AddressSpace::Generic);

            let aspen_main = module.add_function("aspen_main", void_type.fn_type(&[i8_pointer_type.into()], false), Some(Linkage::External));

            {
                let main_fn = module.add_function("main", void_type.fn_type(&[], false), None);
                let entry_block = context.append_basic_block(main_fn, "entry");

                let builder = context.create_builder();
                builder.position_at_end(entry_block);

                let name = builder.build_global_string_ptr(aspen_module.uri().short_name(), "");
                builder.build_call(aspen_main, &[name.as_pointer_value().into()], "");
                builder.build_return(None);
            }
        }
    }

    let buffer = module.write_bitcode_to_memory();

    let mut compile = std::process::Command::new("/usr/local/opt/llvm/bin/llc")
        .stdin(Stdio::piped())
        .arg("-filetype=obj")
        .arg("-o")
        .arg(object_path.to_str().unwrap())
        .spawn()?;

    compile.stdin.as_mut().unwrap().write_all(buffer.as_slice())?;

    if !compile.wait()?.success() {
        println!("Failed to compile!");
        return Ok(());
    }

    Ok(())
}

pub async fn emit_main(modules: Vec<Arc<AModule>>) -> Result<()> {
    let mut bin_path = std::env::current_dir()?;
    let project_name = bin_path.file_name().unwrap().to_os_string();
    bin_path.push(project_name);

    let mut runtime_path = current_exe()?;
    runtime_path.pop();
    runtime_path.push("libaspen.a");

    let mut ld = tokio::process::Command::new("/usr/local/opt/llvm/bin/ld64.lld");

    ld
        .arg("-sdk_version")
        .arg("10.0.0")
        .arg("-arch")
        .arg(ARCH)
        .arg("-lSystem")
        .arg(runtime_path);

    for module in modules {
        if let Some(object_path) = module.object_file_path() {
            ld.arg(object_path.to_str().unwrap());
        }
    }

    let status = ld
        .arg("-o")
        .arg(bin_path.to_str().unwrap())
        .spawn()?
        .await?;

    if !status.success() {
        println!("Failed to link!");
        return Ok(());
    }

    println!("Compiled {}", bin_path.to_str().unwrap());

    Ok(())
}
