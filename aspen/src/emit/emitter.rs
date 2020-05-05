use crate::emit::{EmissionContext, JITResult, OutputResult, JIT};
use crate::semantics::Module;
use crate::syntax::Node;
use crate::SourceKind;
use inkwell::builder::Builder;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetTriple,
};
use inkwell::types::StructType;
use inkwell::values::{FunctionValue, PointerValue, StructValue};
use inkwell::OptimizationLevel;
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

pub struct Emitter<'ctx> {
    context: &'ctx EmissionContext,
    llvm_module: inkwell::module::Module<'ctx>,
    module: Arc<Module>,

    object_type: StructType<'ctx>,
}

const TARGET: &str = env!("TARGET");

static ID_GEN: AtomicUsize = AtomicUsize::new(0);

fn new_id() -> usize {
    ID_GEN.fetch_add(1, Ordering::SeqCst)
}

fn new_name() -> String {
    let mut name = "gen".to_string();
    name.push_str(format!("{}", new_id()).as_str());
    name
}

impl<'ctx> Emitter<'ctx> {
    pub fn new(context: &'ctx EmissionContext, module: Arc<Module>) -> Emitter<'ctx> {
        let m = context.inner.create_module(module.uri().as_ref());

        let object_type = context.inner.opaque_struct_type("Object");
        object_type.set_body(&[], false);

        Emitter {
            context,
            llvm_module: m,
            module,

            object_type,
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
                self.emit_module(module_node).await;
            }
            SourceKind::Inline => {
                let inline_node = self.module.syntax_tree().clone();
                self.emit_inline(inline_node).await;
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

    pub async fn evaluate(&mut self) -> JITResult<()> {
        let mut jit = JIT::new(&self.context.inner, &self.llvm_module);
        match self.module.kind() {
            SourceKind::Module => {
                let module_node = self.module.syntax_tree().clone();
                self.emit_module(module_node).await;
                Ok(())
            }
            SourceKind::Inline => {
                let inline_node = self.module.syntax_tree().clone();
                if let Some(f) = self.emit_inline(inline_node).await {
                    unsafe {
                        jit.evaluate(f)?;
                    }
                }
                Ok(())
            }
        }
    }

    async fn emit_module(&mut self, _module: Arc<Node>) {}

    async fn emit_inline(&mut self, inline: Arc<Node>) -> Option<FunctionValue<'ctx>> {
        if inline.kind.is_expression() {
            let expression_fn = self.llvm_module.add_function(
                new_name().as_str(),
                self.object_type.fn_type(&[], false),
                None,
            );
            let builder = self.context.inner.create_builder();
            let entry_block = self
                .context
                .inner
                .append_basic_block(expression_fn, "entry");
            builder.position_at_end(entry_block);
            builder.build_return(Some(&self.emit_expression(&builder, inline).await));
            println!("{}", self.llvm_module.print_to_string().to_string());
            Some(expression_fn)
        } else {
            self.emit_declaration(inline).await;
            None
        }
    }

    async fn emit_expression(
        &mut self,
        builder: &Builder<'ctx>,
        _node: Arc<Node>,
    ) -> StructValue<'ctx> {
        self.object_type.const_zero()
    }

    async fn emit_declaration(&mut self, _node: Arc<Node>) {}
}

impl<'ctx> fmt::Debug for Emitter<'ctx> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EMITTED MODULE")
    }
}
