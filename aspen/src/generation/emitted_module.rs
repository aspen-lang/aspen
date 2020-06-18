use crate::generation::*;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Module;
use inkwell::values::FunctionValue;
use std::fmt;

pub struct EmittedModule<'ctx> {
    pub module: Module<'ctx>,
    intrinsics: Intrinsics<'ctx>,
    init_fn: Option<FunctionValue<'ctx>>,
}

impl<'ctx> EmittedModule<'ctx> {
    pub fn new(module: Module<'ctx>, intrinsics: Intrinsics<'ctx>) -> EmittedModule<'ctx> {
        EmittedModule {
            module,
            intrinsics,
            init_fn: None,
        }
    }

    pub fn new_executable(
        module: Module<'ctx>,
        intrinsics: Intrinsics<'ctx>,
        init_fn: FunctionValue<'ctx>,
    ) -> EmittedModule<'ctx> {
        EmittedModule {
            module,
            intrinsics,
            init_fn: Some(init_fn),
        }
    }

    pub unsafe fn evaluate(&self, engine: ExecutionEngine<'ctx>) {
        engine.add_module(&self.module).unwrap_or(());

        self.intrinsics.map_in_jit(&engine);

        if let Some(init_fn) = &self.init_fn {
            engine.run_function(*init_fn, &[]);
        }
    }

    pub fn verify(&self) -> GenResult<()> {
        match self.module.verify() {
            Ok(()) => Ok(()),
            Err(s) => Err(GenError::LLVM(format!("{:?}{}", self, s.to_string()))),
        }
    }
}

impl<'ctx> fmt::Debug for EmittedModule<'ctx> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            self.module.print_to_string().as_ref().to_string_lossy()
        )
    }
}
