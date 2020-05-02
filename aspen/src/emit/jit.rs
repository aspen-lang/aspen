use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, FunctionLookupError};
use inkwell::module::Module;
use inkwell::values::PointerValue;
use inkwell::OptimizationLevel;
use std::error::Error;
use std::fmt;

pub struct JIT<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    engine: ExecutionEngine<'ctx>,
}

impl<'ctx> JIT<'ctx> {
    pub fn new(context: &'ctx Context, module: Module<'ctx>) -> JIT<'ctx> {
        ExecutionEngine::link_in_mc_jit();

        let engine = module
            .create_jit_execution_engine(OptimizationLevel::Default)
            .unwrap();

        JIT {
            context,
            module,
            engine,
        }
    }

    pub unsafe fn evaluate<F: FnOnce(&Builder<'ctx>) -> PointerValue<'ctx>>(
        &mut self,
        f: F,
    ) -> JITResult<()> {
        let void_type = self.context.void_type();

        let fn_value = self
            .module
            .add_function("eval", void_type.fn_type(&[], false), None);

        let entry_block = self.context.append_basic_block(fn_value, "entry");

        let builder = self.context.create_builder();
        builder.position_at_end(entry_block);

        let value = f(&builder);
        builder.build_return(Some(&value));

        let jitfn = self
            .engine
            .get_function::<unsafe extern "C" fn() -> Ptr>(fn_value.get_name().to_str().unwrap())?;

        println!("Object at: {:?}", jitfn.call());

        Ok(())
    }
}

type Ptr = *const u8;

pub type JITResult<T> = Result<T, JITError>;

#[derive(Debug)]
pub enum JITError {
    FunctionLookup(FunctionLookupError),
}

impl Error for JITError {}

impl fmt::Display for JITError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<FunctionLookupError> for JITError {
    fn from(error: FunctionLookupError) -> Self {
        JITError::FunctionLookup(error)
    }
}
