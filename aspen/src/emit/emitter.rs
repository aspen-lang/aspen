use crate::emit::{EmissionContext, JITResult, JIT};
use crate::semantics::Module;
use crate::syntax::Node;
use crate::SourceKind;
use inkwell::builder::Builder;
use inkwell::values::PointerValue;
use std::fmt;
use std::sync::Arc;
use std::io;

pub struct Emitter<'ctx> {
    context: &'ctx EmissionContext,
    llvm_module: inkwell::module::Module<'ctx>,
    module: Arc<Module>,
}

impl<'ctx> Emitter<'ctx> {
    pub fn new(context: &'ctx EmissionContext, module: Arc<Module>) -> Emitter<'ctx> {
        let m = context.inner.create_module(module.uri().as_ref());

        Emitter {
            context,
            llvm_module: m,
            module,
        }
    }

    pub fn compile(&mut self) -> io::Result<()> {
        match self.module.kind() {
            SourceKind::Module => {
                let module_node = self.module.syntax_tree().clone();
                self.emit_module(module_node);
                Ok(())
            }
            SourceKind::Expression => {
                let expression_node = self.module.syntax_tree().clone();
                Ok(())
            }
        }
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
