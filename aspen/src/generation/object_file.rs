use crate::generation::{GenError, GenResult};
use crate::semantics::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetTriple,
};
use inkwell::OptimizationLevel;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::create_dir_all;
use crate::generation::compile::Compile;

const TARGET: &str = env!("TARGET");

pub struct ObjectFile {
    pub path: PathBuf,
}

impl ObjectFile {
    pub async fn new(module: Arc<Module>) -> GenResult<ObjectFile> {
        let path = module.host.context.object_file_path(module.uri())?;

        let context = inkwell::context::Context::create();
        let llvm_module = context.create_module(module.uri().as_ref());
        let builder = context.create_builder();

        module.compile(&context, &llvm_module, &builder);

        Self::write(path, llvm_module).await
    }

    pub(crate) async fn write(
        path: PathBuf,
        module: inkwell::module::Module<'_>,
    ) -> GenResult<ObjectFile> {
        create_dir_all(path.parent().unwrap()).await?;

        Target::initialize_all(&InitializationConfig::default());
        let triple = TargetTriple::create(TARGET);
        let target = Target::from_triple(&triple)?;
        let machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Aggressive,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or(GenError::NoTargetMachine(triple))?;

        machine.write_to_file(&module, FileType::Object, &path)?;

        Ok(ObjectFile { path })
    }
}
