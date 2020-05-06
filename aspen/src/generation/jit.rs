use crate::generation::compile::Compile;
use crate::generation::{GenError, GenResult};
use crate::semantics::Module;
use crate::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::OptimizationLevel;
use std::sync::Arc;
use std::sync::Mutex;

lazy_static! {
    static ref LOCK: Mutex<()> = Mutex::new(());
}

static mut CONTEXT: Option<inkwell::context::Context> = None;

pub struct JIT {
    engine: ExecutionEngine<'static>,
}

impl JIT {
    pub fn new(_context: Arc<Context>) -> JIT {
        unsafe {
            if CONTEXT.is_none() {
                let lock = LOCK.lock();
                CONTEXT = Some(inkwell::context::Context::create());
                drop(lock);
            }
            let module = CONTEXT.as_ref().unwrap().create_module("JIT");
            let engine = module
                .create_jit_execution_engine(OptimizationLevel::Default)
                .unwrap();

            JIT { engine }
        }
    }

    pub fn evaluate(&self, module: Arc<Module>) -> GenResult<()> {
        unsafe {
            let context = CONTEXT.as_ref().unwrap();
            let llvm_module = context.create_module(module.uri().as_ref());
            let builder = context.create_builder();

            let f = module.compile(context, &llvm_module, &builder)?;
            if cfg!(debug_assertions) {
                eprintln!("\n\n");
                llvm_module.print_to_stderr();
                eprintln!("\n\n");
            }

            self.engine
                .add_module(&llvm_module)
                .map_err(|()| GenError::UndefinedReference)?;

            {
                let jit_fn = self
                    .engine
                    .get_function::<unsafe extern "C" fn()>(f.get_name().to_str().unwrap())
                    .unwrap();
                jit_fn.call();
            }
        }
        Ok(())
    }
}
