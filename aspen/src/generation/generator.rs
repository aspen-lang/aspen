use crate::generation::GenResult;
use crate::semantics::types::Type;
use crate::semantics::{Host, Module as HostModule};
use crate::syntax;
use futures::executor::block_on;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::types::{FunctionType, PointerType, VoidType};
use inkwell::values::{BasicValueEnum, FunctionValue};
use inkwell::AddressSpace;
use std::fmt;
use std::os::raw::c_char;
use std::sync::Arc;

pub struct Generator<'ctx> {
    context: &'ctx Context,
    #[allow(unused)]
    host: Host,

    // void
    void_type: VoidType<'ctx>,
    // *i8
    str_type: PointerType<'ctx>,

    // () -> void
    void_fn_type: FunctionType<'ctx>,
    // () -> i32
    main_fn_type: FunctionType<'ctx>,
}

impl<'ctx> Generator<'ctx> {
    pub fn new(host: Host, context: &'ctx Context) -> Generator<'ctx> {
        let void_type = context.void_type();
        let i32_type = context.i32_type();
        let i8_type = context.i8_type();
        let str_type = i8_type.ptr_type(AddressSpace::Generic);

        let void_fn_type = void_type.fn_type(&[], false);
        let main_fn_type = i32_type.fn_type(&[], false);

        Generator {
            context,
            host,
            void_type,
            str_type,
            void_fn_type,
            main_fn_type,
        }
    }

    pub fn generate_module(&self, module: &Arc<HostModule>) -> GenResult<EmittedModule> {
        let llvm_module = self.context.create_module(module.uri().as_ref());

        let init_fn = self.generate_root(module, &llvm_module, module.syntax_tree())?;

        Ok(EmittedModule {
            module: llvm_module,
            init_fn,
        })
    }

    pub fn generate_main(&self, main: &str) -> GenResult<EmittedModule> {
        let context = self.context;
        let module = context.create_module("main");
        let builder = context.create_builder();
        let main_fn = module.add_function("main", self.main_fn_type, None);
        let entry_block = context.append_basic_block(main_fn, "entry");
        builder.position_at_end(entry_block);

        let main_type = context.opaque_struct_type(main);
        let main_init_fn = module.add_function(
            format!("{}::New", main).as_str(),
            self.void_type
                .fn_type(&[main_type.ptr_type(AddressSpace::Generic).into()], false),
            Some(Linkage::External),
        );
        let main_to_string_fn = module.add_function(
            format!("{}::ToString", main).as_str(),
            context
                .i8_type()
                .ptr_type(AddressSpace::Generic)
                .fn_type(&[main_type.into()], false),
            Some(Linkage::External),
        );
        let print_fn = self.print_fn(&module);

        let main_obj = builder.build_alloca(main_type, "main_obj");
        builder.build_call(main_init_fn, &[], "");
        let object_as_string =
            builder.build_call(main_to_string_fn, &[main_obj.into()], "object_as_string");
        builder.build_call(
            print_fn,
            &[object_as_string.try_as_basic_value().left().unwrap()],
            "",
        );

        let status_code = context.i32_type().const_int(13, false);
        builder.build_return(Some(&status_code));

        Ok(EmittedModule {
            module,
            init_fn: Some(main_fn),
        })
    }

    fn print_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[link(name = "aspen_runtime")]
        extern "C" {
            fn print(message: *mut c_char);
        }
        {
            #[cfg(not(test))]
            #[used]
            static USED: unsafe extern "C" fn(*mut c_char) = print;
        }

        module.get_function("print").unwrap_or_else(|| {
            module.add_function(
                "print",
                self.void_type.fn_type(&[self.str_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn generate_root(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        root: &Arc<syntax::Root>,
    ) -> GenResult<Option<FunctionValue<'ctx>>> {
        match root.as_ref() {
            syntax::Root::Module(syntax_module) => {
                self.generate_syntax_module(host_module, module, syntax_module)?;
                Ok(None)
            }
            syntax::Root::Inline(inline) => self.generate_inline(host_module, module, inline),
        }
    }

    fn generate_syntax_module(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        syntax_module: &Arc<syntax::Module>,
    ) -> GenResult<()> {
        for declaration in syntax_module.declarations.iter() {
            self.generate_declaration(host_module, module, declaration)?;
        }
        Ok(())
    }

    fn generate_inline(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        inline: &Arc<syntax::Inline>,
    ) -> GenResult<Option<FunctionValue<'ctx>>> {
        match inline.as_ref() {
            syntax::Inline::Expression(expression) => {
                let builder = self.context.create_builder();

                let run_fn =
                    module.add_function("run_inline", self.void_fn_type, Some(Linkage::External));
                {
                    let entry_block = self.context.append_basic_block(run_fn, "entry");
                    builder.position_at_end(entry_block);

                    let object =
                        self.generate_expression(host_module, module, &builder, expression)?;

                    let to_string_fn_name = format!(
                        "{}::ToString",
                        object
                            .get_type()
                            .into_pointer_type()
                            .get_element_type()
                            .into_struct_type()
                            .get_name()
                            .unwrap()
                            .to_string_lossy()
                    );
                    let to_string_fn = module
                        .get_function(to_string_fn_name.as_str())
                        .unwrap_or_else(|| {
                            module.add_function(
                                to_string_fn_name.as_str(),
                                self.str_type.fn_type(&[object.get_type()], false),
                                Some(Linkage::External),
                            )
                        });

                    let as_string = builder.build_call(to_string_fn, &[object], "as_string");
                    let print_fn = self.print_fn(module);

                    builder.build_call(
                        print_fn,
                        &[as_string.try_as_basic_value().left().unwrap()],
                        "",
                    );

                    builder.build_return(None);
                }

                Ok(Some(run_fn))
            }
            syntax::Inline::Declaration(declaration) => {
                self.generate_declaration(host_module, module, declaration)?;
                Ok(None)
            }
        }
    }

    fn generate_expression(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        expression: &Arc<syntax::Expression>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let type_ = block_on(host_module.get_type_of(expression.clone()));
        match expression.as_ref() {
            syntax::Expression::Reference(r) => {
                self.generate_reference_expression(module, builder, r, type_)
            }
        }
    }

    fn generate_reference_expression(
        &self,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        _expression: &Arc<syntax::ReferenceExpression>,
        type_: Type,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        match type_ {
            Type::Object(o) => {
                let symbol = o.symbol();

                let type_ = module.get_struct_type(symbol).unwrap();

                let new_fn_name = format!("{}::New", symbol);
                let new_fn = module
                    .get_function(new_fn_name.as_str())
                    .unwrap_or_else(|| {
                        module.add_function(
                            new_fn_name.as_str(),
                            self.void_type
                                .fn_type(&[type_.ptr_type(AddressSpace::Generic).into()], false),
                            Some(Linkage::External),
                        )
                    });

                let instance = builder.build_alloca(type_, "instance");
                builder.build_call(new_fn, &[instance.into()], "");
                Ok(instance.into())
            }
            t => unimplemented!("generation for references to {:?}", t),
        }
    }

    fn generate_declaration(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        declaration: &Arc<syntax::Declaration>,
    ) -> GenResult<()> {
        match declaration.as_ref() {
            syntax::Declaration::Object(d) => {
                self.generate_object_declaration(host_module, module, d)
            }
            syntax::Declaration::Class(d) => self.generate_class_declaration(d),
            syntax::Declaration::Instance(_) => Ok(()),
        }
    }

    fn generate_object_declaration(
        &self,
        _host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        declaration: &Arc<syntax::ObjectDeclaration>,
    ) -> GenResult<()> {
        let qn = declaration.symbol();

        let type_ = self.context.opaque_struct_type(qn);
        type_.set_body(&[], false);

        let builder = self.context.create_builder();

        let init_fn_name = format!("{}::New", qn);
        let init_fn = module.add_function(
            init_fn_name.as_str(),
            self.void_type
                .fn_type(&[type_.ptr_type(AddressSpace::Generic).into()], false),
            Some(Linkage::External),
        );
        {
            let entry_block = self.context.append_basic_block(init_fn, "entry");
            builder.position_at_end(entry_block);

            let _instance = init_fn.get_first_param().unwrap();
            // TODO: Initialize all fields on instance

            builder.build_return(None);
        }

        let to_string_fn_name = format!("{}::ToString", qn);
        let to_string_fn = module.add_function(
            to_string_fn_name.as_str(),
            self.str_type.fn_type(&[type_.into()], false),
            Some(Linkage::External),
        );
        {
            let entry_block = self.context.append_basic_block(to_string_fn, "entry");
            builder.position_at_end(entry_block);

            let _instance = to_string_fn.get_first_param().unwrap();
            // TODO: Recursively call the ToString method for each field on the instance

            let as_string = builder.build_global_string_ptr(qn, "as_string");

            builder.build_return(Some(&as_string));
        }
        Ok(())
    }

    fn generate_class_declaration(
        &self,
        _declaration: &Arc<syntax::ClassDeclaration>,
    ) -> GenResult<()> {
        Ok(())
    }
}

pub struct EmittedModule<'ctx> {
    pub module: Module<'ctx>,
    // Either () -> i32
    init_fn: Option<FunctionValue<'ctx>>,
}

impl<'ctx> EmittedModule<'ctx> {
    pub unsafe fn evaluate(&self, engine: ExecutionEngine<'ctx>) {
        engine.add_module(&self.module).unwrap_or(());
        if let Some(init_fn) = &self.init_fn {
            engine.run_function(*init_fn, &[]);
        }
    }
}

impl<'ctx> fmt::Debug for EmittedModule<'ctx> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            self.module.print_to_string().as_ref().to_string_lossy()
        )
    }
}
