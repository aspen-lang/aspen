use crate::generation::{GenError, GenResult};
use crate::syntax::Node;
use crate::{semantics, syntax};
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::types::StructType;
use inkwell::values::{BasicValueEnum, FunctionValue};
use inkwell::AddressSpace;
use std::os::raw::c_char;
use std::sync::Arc;

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

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        self.syntax_tree().compile(context, module, builder)
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Root> {
    type Output = FunctionValue<'ctx>;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        match &**self {
            syntax::Root::Module(n) => n.compile(context, module, builder),
            syntax::Root::Inline(n) => {
                let inline_fn = module.add_function(
                    format!("{:?}::inline", n.source().uri()).as_str(),
                    context.void_type().fn_type(&[], false),
                    None,
                );
                let entry_block = context.append_basic_block(inline_fn, "entry");
                builder.position_at_end(entry_block);
                let result = n.compile(context, module, builder);
                match result {
                    Ok(Some(result)) => {
                        let result_type = result.get_type().into_struct_type();
                        let type_name = result_type.get_name().unwrap();
                        let to_string_fn_name =
                            format!("{}::ToString", type_name.to_str().unwrap());
                        let to_string_fn = module
                            .get_function(to_string_fn_name.as_str())
                            .unwrap_or_else(|| {
                                module.add_function(
                                    to_string_fn_name.as_str(),
                                    context
                                        .i8_type()
                                        .ptr_type(AddressSpace::Generic)
                                        .fn_type(&[result.get_type()], false),
                                    Some(Linkage::External),
                                )
                            });

                        let as_string = builder.build_call(to_string_fn, &[result], "");

                        let print = Print.compile(context, module, builder)?;
                        builder.build_call(
                            print,
                            &[as_string.try_as_basic_value().left().unwrap()],
                            "",
                        );
                        builder.build_return(None);
                        Ok(inline_fn)
                    }
                    Ok(None) => {
                        builder.build_return(None);
                        Ok(inline_fn)
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Module> {
    type Output = FunctionValue<'ctx>;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        let fn_name = format!("{:?}::init", self.source.uri());
        let init_fn = module.add_function(
            fn_name.as_str(),
            context.void_type().fn_type(&[], false),
            Some(Linkage::External),
        );
        let entry_block = context.append_basic_block(init_fn, "entry");
        builder.position_at_end(entry_block);

        for declaration in self.declarations.iter() {
            declaration.compile(context, module, builder)?;
        }

        builder.build_return(None);

        Ok(init_fn)
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Inline> {
    type Output = Option<BasicValueEnum<'ctx>>;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        match self.as_ref() {
            syntax::Inline::Declaration(n) => {
                n.compile(context, module, builder)?;
                Ok(None)
            }
            syntax::Inline::Expression(n) => n.compile(context, module, builder).map(Some),
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Declaration> {
    type Output = ();

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        match self.as_ref() {
            syntax::Declaration::Object(n) => {
                n.compile(context, module, builder)?;
                Ok(())
            }
            syntax::Declaration::Class(n) => n.compile(context, module, builder),
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::ObjectDeclaration> {
    type Output = StructType<'ctx>;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        _builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        let type_ = context.opaque_struct_type(self.symbol());
        type_.set_body(&[], false);

        let builder = context.create_builder();

        // INIT FUNCTION
        let init_fn = module.add_function(
            self.symbol(),
            type_.fn_type(&[], false),
            Some(Linkage::External),
        );
        let entry_block = context.append_basic_block(init_fn, "entry");
        builder.position_at_end(entry_block);

        let object = type_.const_zero();
        builder.build_return(Some(&object));

        // TOSTRING FUNCTION
        let to_string_fn = module.add_function(
            format!("{}::ToString", self.symbol()).as_str(),
            context
                .i8_type()
                .ptr_type(AddressSpace::Generic)
                .fn_type(&[type_.ptr_type(AddressSpace::Generic).into()], false),
            Some(Linkage::External),
        );
        let entry_block = context.append_basic_block(to_string_fn, "entry");
        builder.position_at_end(entry_block);
        builder.build_return(Some(&builder.build_global_string_ptr(self.symbol(), "")));

        Ok(type_)
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::ClassDeclaration> {
    type Output = ();

    fn compile(
        &self,
        _context: &'ctx Context,
        _module: &Module<'ctx>,
        _builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        // NoOp
        Ok(())
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::Expression> {
    type Output = BasicValueEnum<'ctx>;

    fn compile(
        &self,
        context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        match self.as_ref() {
            syntax::Expression::Reference(n) => n.compile(context, module, builder),
        }
    }
}

impl<'ctx> Compile<'ctx> for Arc<syntax::ReferenceExpression> {
    type Output = BasicValueEnum<'ctx>;

    fn compile(
        &self,
        _context: &'ctx Context,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> GenResult<Self::Output> {
        let type_ = module
            .get_struct_type(self.symbol.identifier.lexeme())
            .ok_or(GenError::UndefinedReference)?;

        let init_fn = module
            .get_function(self.symbol.identifier.lexeme())
            .unwrap_or_else(|| {
                module.add_function(
                    self.symbol.identifier.lexeme(),
                    type_.fn_type(&[], false),
                    Some(Linkage::External),
                )
            });

        let object = builder.build_call(init_fn, &[], "");
        Ok(object.try_as_basic_value().left().unwrap())
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
                context.void_type().fn_type(
                    &[context.i8_type().ptr_type(AddressSpace::Generic).into()],
                    false,
                ),
                Some(Linkage::External),
            )
        }))
    }
}
