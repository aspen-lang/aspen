use crate::generation::compile::Compile;
use crate::generation::GenResult;
use crate::Context;
use inkwell::builder::Builder;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::values::FunctionValue;
use inkwell::OptimizationLevel;
use std::sync::Arc;
use std::sync::Mutex;

lazy_static! {
    static ref LOCK: Mutex<()> = Mutex::new(());
}

static mut CONTEXT: Option<inkwell::context::Context> = None;

pub struct JIT {
    module: inkwell::module::Module<'static>,
    engine: ExecutionEngine<'static>,
    builder: Builder<'static>,
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
            let builder = CONTEXT.as_ref().unwrap().create_builder();

            JIT {
                module,
                engine,
                builder,
            }
        }
    }

    pub fn evaluate<C: Compile<'static, Output = FunctionValue<'static>>>(
        &self,
        c: C,
    ) -> GenResult<()> {
        unsafe {
            let f = c.compile(CONTEXT.as_ref().unwrap(), &self.module, &self.builder);
            self.engine.run_function(f, &[]);
        }
        Ok(())
    }
}
