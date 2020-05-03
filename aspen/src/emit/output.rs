use inkwell::support::LLVMString;
use std::io;

pub type OutputResult<T> = Result<T, OutputError>;

#[derive(Debug)]
pub enum OutputError {
    IO(io::Error),
    LLVM(LLVMString),
    FailedToLink,
}

impl From<io::Error> for OutputError {
    fn from(err: io::Error) -> Self {
        OutputError::IO(err)
    }
}

impl From<LLVMString> for OutputError {
    fn from(err: LLVMString) -> Self {
        OutputError::LLVM(err)
    }
}
