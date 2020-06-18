use futures::io::Error;
use inkwell::support::LLVMString;
use inkwell::targets::TargetTriple;
use std::fmt;
use std::io;

pub type GenResult<T> = Result<T, GenError>;

pub enum GenError {
    Multi(Vec<GenError>),
    IO(io::Error),
    FailedToLink(String),
    NoTargetMachine(TargetTriple),
    LLVM(String),
    UndefinedReference,
    BadNode,
    InvalidMainObject(String),
}

impl fmt::Debug for GenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use GenError::*;
        match self {
            Multi(errs) => {
                for (i, e) in errs.iter().enumerate() {
                    if i > 0 {
                        write!(f, "\n")?;
                    }
                    write!(f, "{:?}", e)?;
                }
                Ok(())
            }
            IO(e) => fmt::Debug::fmt(e, f),
            FailedToLink(s) => write!(f, "Failed to link: {}", s),
            NoTargetMachine(t) => write!(f, "No such target machine: {:?}", t),
            LLVM(s) => fmt::Display::fmt(s, f),
            UndefinedReference => write!(f, "Undefined reference"),
            BadNode => write!(f, "Bad node"),
            InvalidMainObject(s) => fmt::Display::fmt(s, f),
        }
    }
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
