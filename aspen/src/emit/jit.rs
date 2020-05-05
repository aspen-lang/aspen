use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, FunctionLookupError};
use inkwell::module::Module;
use inkwell::values::FunctionValue;
use inkwell::OptimizationLevel;
use std::error::Error;
use std::fmt;

pub struct JIT<'ctx> {
    engine: ExecutionEngine<'ctx>,
}

impl<'ctx> JIT<'ctx> {
    pub fn new(_context: &'ctx Context, module: &Module<'ctx>) -> JIT<'ctx> {
        ExecutionEngine::link_in_mc_jit();

        let engine = module
            .create_jit_execution_engine(OptimizationLevel::Default)
            .unwrap();

        JIT {
            engine,
        }
    }

    pub unsafe fn evaluate(&mut self, f: FunctionValue<'ctx>) -> JITResult<()> {
        println!("Lookup {:?}", f.get_name());
        let jitfn = self
            .engine
            .get_function::<unsafe extern "C" fn() -> Ptr>(f.get_name().to_str().unwrap())?;

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
