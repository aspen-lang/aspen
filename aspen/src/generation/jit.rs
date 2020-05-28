use crate::generation::{GenResult, Generator};
use crate::semantics::{Host, Module};
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
            let context = CONTEXT.as_ref().unwrap();
            let module = context.create_module("JIT");
            let engine = module
                .create_jit_execution_engine(OptimizationLevel::Default)
                .unwrap();

            JIT { engine }
        }
    }

    pub fn evaluate(&self, module: Arc<Module>) -> GenResult<()> {
        unsafe {
            let generator = Generator::new(module.host.clone(), CONTEXT.as_ref().unwrap());
            let module = generator.generate_module(&module)?;

            if cfg!(debug_assertions) {
                eprintln!("------------------\n{:?}------------------", module);
            }

            module.evaluate(&generator, self.engine.clone());
        }
        Ok(())
    }

    pub fn evaluate_main<M: AsRef<str>>(self, host: Host, main: M) -> GenResult<()> {
        unsafe {
            let generator = Generator::new(host.clone(), CONTEXT.as_ref().unwrap());
            let module = generator.generate_main(main.as_ref())?;

            if cfg!(debug_assertions) {
                eprintln!("------------------\n{:?}------------------", module);
            }

            module.evaluate(&generator, self.engine.clone());
        }
        Ok(())
    }
}
