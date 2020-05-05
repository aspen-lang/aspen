use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::values::{FunctionValue, IntValue};
use std::sync::Arc;
use crate::{syntax, semantics};

pub trait Compile<'ctx> {
    type Output;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> Self::Output;
}

impl<'ctx> Compile<'ctx> for Arc<semantics::Module> {
    type Output = FunctionValue<'ctx>;

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> Self::Output {
        self.syntax_tree().compile(context, module, builder)
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Root> {
    type Output = FunctionValue<'ctx>;

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> Self::Output {
        match &**self {
            syntax::Root::Module(n) => n.compile(context, module, builder),
            syntax::Root::Inline(n) => n.compile(context, module, builder),
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Module> {
    type Output = FunctionValue<'ctx>;

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> Self::Output {
        let fn_name = format!("{:?}::init", self.source.uri());
        let init_fn = module.add_function(fn_name.as_str(), context.void_type().fn_type(&[], false), Some(Linkage::External));
        let entry_block = context.append_basic_block(init_fn, "entry");
        builder.position_at_end(entry_block);
        builder.build_call(HelloWorld.compile(context, module, builder), &[], "");
        builder.build_return(None);

        init_fn
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Inline> {
    type Output = FunctionValue<'ctx>;

    fn compile(&self, _context: &'ctx Context, _module: &Module<'ctx>, _builder: &Builder<'ctx>) -> Self::Output {
        unimplemented!("Compile Inline")
    }
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
