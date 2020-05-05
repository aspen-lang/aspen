use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::values::{FunctionValue, BasicValueEnum};
use std::sync::Arc;
use crate::{syntax, semantics};
use std::os::raw::c_char;
use inkwell::AddressSpace;
use inkwell::types::StructType;
use crate::generation::{GenResult, GenError};

pub trait Compile<'ctx> {
    type Output;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output>;
}

impl<'ctx> Compile<'ctx> for Arc<semantics::Module> {
    type Output = FunctionValue<'ctx>;

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        self.syntax_tree().compile(context, module, builder)
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Root> {
    type Output = FunctionValue<'ctx>;

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        match &**self {
            syntax::Root::Module(n) => n.compile(context, module, builder),
            syntax::Root::Inline(n) => {
                let inline_fn = module.add_function("inline", context.void_type().fn_type(&[], false), None);
                let entry_block = context.append_basic_block(inline_fn, "entry");
                builder.position_at_end(entry_block);
                n.compile(context, module, builder)?;
                builder.build_return(None);
                Ok(inline_fn)
            },
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Module> {
    type Output = FunctionValue<'ctx>;

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        let fn_name = format!("{:?}::init", self.source.uri());
        let init_fn = module.add_function(fn_name.as_str(), context.void_type().fn_type(&[], false), Some(Linkage::External));
        let entry_block = context.append_basic_block(init_fn, "entry");
        builder.position_at_end(entry_block);
        builder.build_call(Print.compile(context, module, builder)?, &[
            builder.build_global_string_ptr(format!("Init {}", self.source.uri()).as_str(), "").as_pointer_value().into(),
        ], "");

        for declaration in self.declarations.iter() {
            declaration.compile(context, module, builder)?;
        }

        builder.build_return(None);

        Ok(init_fn)
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Inline> {
    type Output = ();

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        match self.as_ref() {
            syntax::Inline::Declaration(n) => n.compile(context, module, builder),
            syntax::Inline::Expression(n) => {
                n.compile(context, module, builder)?;
                Ok(())
            },
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Declaration> {
    type Output = ();

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        match self.as_ref() {
            syntax::Declaration::Object(n) => {
                n.compile(context, module, builder)?;
                Ok(())
            },
            syntax::Declaration::Class(n) => n.compile(context, module, builder),
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::ObjectDeclaration> {
    type Output = StructType<'ctx>;

    fn compile(&self, context: &'ctx Context, _module: &Module<'ctx>, _builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        let type_ = context.opaque_struct_type(self.symbol());
        type_.set_body(&[], false);
        Ok(type_)
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::ClassDeclaration> {
    type Output = ();

    fn compile(&self, _context: &'ctx Context, _module: &Module<'ctx>, _builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        // NoOp
        Ok(())
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Expression> {
    type Output = BasicValueEnum<'ctx>;

    fn compile(&self, context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        match self.as_ref() {
            syntax::Expression::Reference(n) => n.compile(context, module, builder),
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::ReferenceExpression> {
    type Output = BasicValueEnum<'ctx>;

    fn compile(&self, _context: &'ctx Context, module: &Module<'ctx>, builder: &Builder<'ctx>) -> GenResult<Self::Output> {
        let type_ = module.get_struct_type(self.symbol.identifier.lexeme()).ok_or(GenError::UndefinedReference)?;
        let object = builder.build_alloca(type_, "");
        Ok(object.into())
    }
}

pub struct Print;

impl<'ctx> Compile<'ctx> for Print {
    type Output = FunctionValue<'ctx>;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        _builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        #[link(name = "aspen_runtime")]
        extern "C" {
            fn print(message: *mut c_char);
        }
        {
            #[used]
            static USED: unsafe extern "C" fn(*mut c_char) = print;
        }

        Ok(module.get_function("print").unwrap_or_else(|| {
            module.add_function(
                "print",
                context.i8_type().ptr_type(AddressSpace::Generic).fn_type(&[], false),
                Some(Linkage::External),
            )
        }))
    }
}
