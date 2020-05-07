use crate::generation::{EmittedModule, GenError, GenResult, Generator};
use crate::semantics::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetTriple,
};
use inkwell::OptimizationLevel;
use std::path::PathBuf;
use std::sync::Arc;

const TARGET: &str = env!("TARGET");

pub struct ObjectFile {
    pub path: PathBuf,
}

impl ObjectFile {
    pub async fn new(module: Arc<Module>) -> GenResult<ObjectFile> {
        let path = module.host.context.object_file_path(module.uri())?;
        let context = inkwell::context::Context::create();

        let generator = Generator::new(module.host.clone(), &context);
        let emitted = generator.generate_module(&module)?;

        module.host.context.ensure_object_file_dir().await?;
        Self::write(path, emitted).await
    }

    pub(crate) async fn write(path: PathBuf, module: EmittedModule<'_>) -> GenResult<ObjectFile> {
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

        machine.write_to_file(&module.module, FileType::Object, &path)?;

        Ok(ObjectFile { path })
    }
}
