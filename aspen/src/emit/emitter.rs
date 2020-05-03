use crate::emit::{EmissionContext, JITResult, OutputResult, JIT};
use crate::semantics::Module;
use crate::syntax::Node;
use crate::SourceKind;
use inkwell::builder::Builder;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetTriple,
};
use inkwell::values::PointerValue;
use inkwell::OptimizationLevel;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

pub struct Emitter<'ctx> {
    context: &'ctx EmissionContext,
    llvm_module: inkwell::module::Module<'ctx>,
    module: Arc<Module>,
}

const TARGET: &str = env!("TARGET");

impl<'ctx> Emitter<'ctx> {
    pub fn new(context: &'ctx EmissionContext, module: Arc<Module>) -> Emitter<'ctx> {
        let m = context.inner.create_module(module.uri().as_ref());

        Emitter {
            context,
            llvm_module: m,
            module,
        }
    }

    pub async fn output(&mut self) -> OutputResult<()> {
        let object_file_path = self.module.object_file_path()?;

        let object_modified = object_file_path
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if object_modified > *self.module.modified() {
            return Ok(());
        }

        match self.module.kind() {
            SourceKind::Module => {
                let module_node = self.module.syntax_tree().clone();
                self.emit_module(module_node);
            }
            SourceKind::Expression => {
                let _expression_node = self.module.syntax_tree().clone();
            }
        }

        Self::write_object(&object_file_path, &self.llvm_module).await?;

        println!("Compiled {:?}", self.module.uri());
        Ok(())
    }

    pub(crate) async fn write_object(
        object_file_path: &Path,
        llvm_module: &inkwell::module::Module<'ctx>,
    ) -> OutputResult<()> {
        tokio::fs::create_dir_all(object_file_path.parent().unwrap()).await?;

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
            .unwrap();

        Ok(machine.write_to_file(llvm_module, FileType::Object, &object_file_path)?)
    }

    pub fn evaluate(&mut self) -> JITResult<()> {
        let mut jit = JIT::new(&self.context.inner, self.llvm_module.clone());
        match self.module.kind() {
            SourceKind::Module => {
                let module_node = self.module.syntax_tree().clone();
                self.emit_module(module_node);
                Ok(())
            }
            SourceKind::Expression => {
                let expression_node = self.module.syntax_tree().clone();
                unsafe { jit.evaluate(|builder| self.emit_expression(builder, expression_node)) }
            }
        }
    }

    fn emit_module(&mut self, _module: Arc<Node>) {}

    fn emit_expression(
        &mut self,
        builder: &Builder<'ctx>,
        _module: Arc<Node>,
    ) -> PointerValue<'ctx> {
        builder
            .build_global_string_ptr("Hello", "")
            .as_pointer_value()
    }
}

impl<'ctx> fmt::Debug for Emitter<'ctx> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EMITTED MODULE")
    }
}
