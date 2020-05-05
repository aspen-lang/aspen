use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::values::{FunctionValue, IntValue};

pub trait Compile<'ctx> {
    type Output;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> Self::Output;
}

pub struct ConstU8(u8);

impl<'ctx> Compile<'ctx> for ConstU8 {
    type Output = IntValue<'ctx>;

    fn compile(
        &self,
        context: &'ctx Context,
        _module: &Module<'ctx>,
        _builder: &Builder<'ctx>,
    ) -> Self::Output {
        context.i8_type().const_int(self.0 as u64, false)
    }
}

pub struct HelloWorld;

impl<'ctx> Compile<'ctx> for HelloWorld {
    type Output = FunctionValue<'ctx>;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        _builder: &Builder<'ctx>,
    ) -> Self::Output {
        #[link(name = "aspen_runtime")]
        extern "C" {
            fn hello_world();
        }
        {
            #[used]
            static USED: unsafe extern "C" fn() = hello_world;
        }

        module.get_function("hello_world").unwrap_or_else(|| {
            module.add_function(
                "hello_world",
                context.void_type().fn_type(&[], false),
                Some(Linkage::External),
            )
        })
    }
}
