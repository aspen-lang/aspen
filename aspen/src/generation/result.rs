use futures::io::Error;
use inkwell::support::LLVMString;
use inkwell::targets::TargetTriple;
use std::io;

pub type GenResult<T> = Result<T, GenError>;

#[derive(Debug)]
pub enum GenError {
    Multi(Vec<GenError>),
    IO(io::Error),
    FailedToLink(String),
    NoTargetMachine(TargetTriple),
    LLVM(String),
    UndefinedReference,
}

impl From<io::Error> for GenError {
    fn from(err: Error) -> Self {
        GenError::IO(err)
    }
}

impl From<LLVMString> for GenError {
    fn from(s: LLVMString) -> Self {
        GenError::LLVM(s.to_string())
    }
}
