use crate::semantics::Module;
use llama::*;
use std::io::Write;
use std::process::Stdio;
use std::sync::Arc;

pub async fn emit_module(aspen_module: &Arc<Module>) -> Result<(), Error> {
    let object_path = match aspen_module.object_file_path() {
        None => return Err(Error::InvalidPath),
        Some(p) => p,
    };

    let context = Context::new()?;
    let module = llama::Module::new(&context, aspen_module.uri().to_string())?;
    let builder = Builder::new(&context)?;

    let void_type = Type::void(&context)?;

    for (name, _) in aspen_module.exported_declarations().await {
        module.declare_function(&builder, name.as_str(), FuncType::new(void_type, &[])?, |_| {
            builder.ret_void()
        })?;
        if name == "Main" {
            let i8_type = Type::i8(&context)?;
            let i8_pointer_type = i8_type.pointer(None)?;

            let aspen_main = module.define_function("aspen_main", FuncType::new(void_type, &[i8_pointer_type])?)?;

            module.declare_function(&builder, "main", FuncType::new(void_type, &[])?, |_| {
                let name = builder.global_string_ptr(aspen_module.uri().short_name(), "")?;
                builder.call(aspen_main, &[name.into()], "")?;
                builder.ret_void()
            })?;
        }
    }

    let buffer = module.write_bitcode_to_memory_buffer()?;

    let mut compile = std::process::Command::new("/usr/local/opt/llvm/bin/llc")
        .stdin(Stdio::piped())
        .arg("-filetype=obj")
        .arg("-o")
        .arg(object_path.to_str().unwrap())
        .spawn()?;

    compile.stdin.as_mut().unwrap().write_all(buffer.as_ref())?;

    if !compile.wait()?.success() {
        println!("Failed to compile!");
        return Ok(());
    }

    Ok(())
}

pub async fn emit_main(modules: Vec<Arc<Module>>) -> Result<(), llama::Error> {
    let mut bin_path = std::env::current_dir()?;
    let project_name = bin_path.file_name().unwrap().to_os_string();
    bin_path.push(project_name);

    let mut ld = tokio::process::Command::new("/usr/local/opt/llvm/bin/ld64.lld");

    ld
        .arg("-sdk_version")
        .arg("10.0.0")
        .arg("-L/usr/lib")
        .arg("-rpath")
        .arg("/usr/local/opt/llvm/lib")
        .arg("-arch")
        .arg("x86_64")
        .arg("/Users/emil.broman/code/aspen-lang/aspen/aspen-runtime/target/release/libaspen.dylib")
        .arg("/usr/lib/libSystem.B.dylib");

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
