use crate::generation::compile::{Compile, Print};
use crate::generation::{GenError, GenResult};
use crate::semantics::Module;
use crate::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Linkage;
use inkwell::{AddressSpace, OptimizationLevel};
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

    pub fn evaluate_main<M: AsRef<str>>(self, main: M) -> GenResult<()> {
        let context = inkwell::context::Context::create();
        let module = context.create_module("main");
        // Main module
        unsafe {
            let builder = context.create_builder();
            let main_fn = module.add_function("main", context.i32_type().fn_type(&[], false), None);
            let entry_block = context.append_basic_block(main_fn, "entry");
            builder.position_at_end(entry_block);

            let main_type = context.opaque_struct_type(main.as_ref());
            let main_init_fn = module.add_function(
                main.as_ref(),
                main_type.fn_type(&[], false),
                Some(Linkage::External),
            );
            let main_to_string_fn = module.add_function(
                format!("{}::ToString", main.as_ref()).as_str(),
                context
                    .i8_type()
                    .ptr_type(AddressSpace::Generic)
                    .fn_type(&[main_type.into()], false),
                Some(Linkage::External),
            );
            let print_fn = Print.compile(&context, &module, &builder)?;

            let main_obj = builder.build_call(main_init_fn, &[], "");
            let object_as_string = builder.build_call(
                main_to_string_fn,
                &[main_obj.try_as_basic_value().left().unwrap()],
                "",
            );
            builder.build_call(
                print_fn,
                &[object_as_string.try_as_basic_value().left().unwrap()],
                "",
            );

            let status_code = context.i32_type().const_int(13, false);
            builder.build_return(Some(&status_code));

            self.engine
                .add_module(&module)
                .map_err(|()| GenError::UndefinedReference)?;

            {
                let jit_fn = self
                    .engine
                    .get_function::<unsafe extern "C" fn()>(main_fn.get_name().to_str().unwrap())
                    .unwrap();
                jit_fn.call();
                drop(self.engine);
            }
        }
        Ok(())
    }
}
