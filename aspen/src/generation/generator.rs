use crate::generation::{GenError, GenResult};
use crate::semantics::types::Type;
use crate::semantics::{Host, Module as HostModule};
use crate::syntax;
use futures::executor::block_on;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::types::{FloatType, FunctionType, IntType, PointerType, VoidType};
use inkwell::values::{BasicValueEnum, FunctionValue};
use inkwell::{AddressSpace, IntPredicate};
use std::fmt;
use std::sync::Arc;

pub struct Generator<'ctx> {
    context: &'ctx Context,
    #[allow(unused)]
    host: Host,

    // void
    void_type: VoidType<'ctx>,
    // usize
    usize_type: IntType<'ctx>,

    // i128
    i128_type: IntType<'ctx>,
    // f64
    f64_type: FloatType<'ctx>,

    // *Value
    value_ptr_type: PointerType<'ctx>,
    // *Reply
    reply_ptr_type: PointerType<'ctx>,

    // () -> void
    void_fn_type: FunctionType<'ctx>,
    // () -> i32
    main_fn_type: FunctionType<'ctx>,
}

impl<'ctx> Generator<'ctx> {
    pub fn new(host: Host, context: &'ctx Context) -> Generator<'ctx> {
        let void_type = context.void_type();
        let i32_type = context.i32_type();
        let usize_type = context.custom_width_int_type((std::mem::size_of::<usize>() * 8) as u32);

        let i128_type = context.i128_type();
        let f64_type = context.f64_type();

        let reply_type = context.opaque_struct_type("Reply");
        let reply_ptr_type = reply_type.ptr_type(AddressSpace::Generic);

        let value_type = context.opaque_struct_type("Value");
        let value_ptr_type = value_type.ptr_type(AddressSpace::Generic);

        let void_fn_type = void_type.fn_type(&[], false);
        let main_fn_type = i32_type.fn_type(&[], false);

        Generator {
            context,
            host,
            void_type,
            usize_type,
            i128_type,
            f64_type,
            value_ptr_type,
            reply_ptr_type,
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

    pub fn generate_main(&self, _main: &str) -> GenResult<EmittedModule> {
        let context = self.context;
        let module = context.create_module("main");
        let builder = context.create_builder();
        let main_fn = module.add_function("main", self.main_fn_type, None);
        let entry_block = context.append_basic_block(main_fn, "entry");
        builder.position_at_end(entry_block);

        let new_object_fn = self.new_object_fn(&module);
        let print_fn = self.print_fn(&module);
        let drop_reference_fn = self.drop_reference_fn(&module);

        let main_obj = builder
            .build_call(new_object_fn, &[], "main_obj")
            .try_as_basic_value()
            .left()
            .unwrap();

        builder.build_call(print_fn, &[main_obj.into()], "");
        builder.build_call(drop_reference_fn, &[main_obj.into()], "");

        let status_code = context.i32_type().const_int(13, false);
        builder.build_return(Some(&status_code));

        Ok(EmittedModule {
            module,
            init_fn: Some(main_fn),
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
            syntax::Inline::Expression(expression, _) => {
                let builder = self.context.create_builder();

                let run_fn =
                    module.add_function("run_inline", self.void_fn_type, Some(Linkage::External));
                {
                    let entry_block = self.context.append_basic_block(run_fn, "entry");
                    builder.position_at_end(entry_block);

                    let mut locals = vec![];

                    let object = self.generate_expression(
                        host_module,
                        module,
                        run_fn,
                        &builder,
                        &mut locals,
                        expression,
                    )?;

                    let print_fn = self.print_fn(module);
                    builder.build_call(print_fn, &[object], "");

                    self.generate_end_of_scope(module, &builder, locals);

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

    fn generate_end_of_scope(
        &self,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        locals: Vec<BasicValueEnum<'ctx>>,
    ) {
        let drop_reference_fn = self.drop_reference_fn(module);

        for local in locals {
            builder.build_call(drop_reference_fn, &[local], "");
        }
    }

    fn generate_expression(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        function: FunctionValue<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        expression: &Arc<syntax::Expression>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let type_ = block_on(host_module.get_type_of(expression.clone()));
        match expression.as_ref() {
            syntax::Expression::Reference(r) => {
                self.generate_reference_expression(module, builder, locals, r, type_)
            }
            syntax::Expression::Integer(i) => self.generate_integer(module, builder, locals, i),
            syntax::Expression::Float(f) => self.generate_float(module, builder, locals, f),
            syntax::Expression::MessageSend(send) => {
                self.generate_message_send(host_module, module, function, builder, locals, send)
            }
        }
    }

    fn generate_integer(
        &self,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        integer: &Arc<syntax::Integer>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        match integer.literal.kind {
            syntax::TokenKind::IntegerLiteral(value, _) => {
                let int_value = builder
                    .build_call(
                        self.new_int_fn(module),
                        &[self
                            .i128_type
                            .const_int_arbitrary_precision(
                                [value as u64, value.wrapping_shr(64) as u64].as_ref(),
                            )
                            .into()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();

                locals.push(int_value);

                Ok(int_value)
            }
            _ => Err(GenError::BadNode),
        }
    }

    fn generate_float(
        &self,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        float: &Arc<syntax::Float>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        match float.literal.kind {
            syntax::TokenKind::FloatLiteral(value, _) => {
                let float_value = builder
                    .build_call(
                        self.new_float_fn(module),
                        &[self.f64_type.const_float(value).into()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                locals.push(float_value);
                Ok(float_value)
            }
            _ => Err(GenError::BadNode),
        }
    }

    fn generate_message_send(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        function: FunctionValue<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        send: &Arc<syntax::MessageSend>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let send_message_fn = self.send_message_fn(module);
        let clone_reference_fn = self.clone_reference_fn(module);
        let poll_reply = self.poll_reply_fn(module);

        let message = self.generate_expression(
            host_module,
            module,
            function,
            builder,
            locals,
            &send.message,
        )?;
        let receiver = self.generate_expression(
            host_module,
            module,
            function,
            builder,
            locals,
            &send.receiver,
        )?;

        builder.build_call(clone_reference_fn, &[message], "");
        builder.build_call(clone_reference_fn, &[receiver], "");

        let reply = builder
            .build_call(send_message_fn, &[receiver, message], "reply")
            .try_as_basic_value()
            .left()
            .unwrap();

        let poll_loop_block = self.context.append_basic_block(function, "poll_loop");

        builder.build_unconditional_branch(poll_loop_block);
        builder.position_at_end(poll_loop_block);

        let object = builder
            .build_call(poll_reply, &[reply], "")
            .try_as_basic_value()
            .left()
            .unwrap();

        let if_reply_is_null = builder.build_int_compare(
            IntPredicate::EQ,
            builder.build_ptr_to_int(object.into_pointer_value(), self.usize_type, ""),
            self.usize_type.const_zero(),
            "if_reply_is_null",
        );

        let exit_block = self
            .context
            .insert_basic_block_after(poll_loop_block, "exit");

        builder.build_conditional_branch(if_reply_is_null, poll_loop_block, exit_block);

        builder.position_at_end(exit_block);

        locals.push(reply);
        locals.push(object);

        Ok(object)
    }

    fn generate_reference_expression(
        &self,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        _expression: &Arc<syntax::ReferenceExpression>,
        type_: Type,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        match type_ {
            Type::Object(_) => {
                let object_ref = builder
                    .build_call(self.new_object_fn(module), &[], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();

                locals.push(object_ref);

                Ok(object_ref)
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
        }
    }

    fn generate_object_declaration(
        &self,
        _host_module: &Arc<HostModule>,
        _module: &Module<'ctx>,
        _declaration: &Arc<syntax::ObjectDeclaration>,
    ) -> GenResult<()> {
        Ok(())
    }
}

pub struct EmittedModule<'ctx> {
    pub module: Module<'ctx>,
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

mod runtime {
    #[repr(C)]
    pub struct Value {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct Reply {
        _private: [u8; 0],
    }

    #[cfg(not(test))]
    #[link(name = "aspen_runtime")]
    extern "C" {
        #[allow(improper_ctypes)]
        pub fn new_int(value: i128) -> *mut Value;
        pub fn new_float(value: f64) -> *mut Value;
        pub fn new_object() -> *mut Value;

        pub fn clone_reference(value: *mut Value);
        pub fn drop_reference(value: *mut Value);

        pub fn send_message(receiver: *mut Value, message: *const Value) -> *mut Reply;
        pub fn poll_reply(reply: *mut Reply) -> *mut Value;

        pub fn print(value: *const Value);
    }
}

impl<'ctx> Generator<'ctx> {
    fn new_int_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static NEW_INT: unsafe extern "C" fn(value: i128) -> *mut runtime::Value = runtime::new_int;

        module.get_function("new_int").unwrap_or_else(|| {
            module.add_function(
                "new_int",
                self.value_ptr_type.fn_type(&[self.i128_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn new_float_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static NEW_FLOAT: unsafe extern "C" fn(value: f64) -> *mut runtime::Value =
            runtime::new_float;

        module.get_function("new_float").unwrap_or_else(|| {
            module.add_function(
                "new_float",
                self.value_ptr_type.fn_type(&[self.f64_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn new_object_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static NEW_OBJECT: unsafe extern "C" fn() -> *mut runtime::Value = runtime::new_object;

        module.get_function("new_object").unwrap_or_else(|| {
            module.add_function(
                "new_object",
                self.value_ptr_type.fn_type(&[], false),
                Some(Linkage::External),
            )
        })
    }

    fn clone_reference_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static CLONE_REFERENCE: unsafe extern "C" fn(value: *mut runtime::Value) =
            runtime::clone_reference;

        module.get_function("clone_reference").unwrap_or_else(|| {
            module.add_function(
                "clone_reference",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn drop_reference_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static DROP_REFERENCE: unsafe extern "C" fn(value: *mut runtime::Value) =
            runtime::drop_reference;

        module.get_function("drop_reference").unwrap_or_else(|| {
            module.add_function(
                "drop_reference",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn send_message_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static SEND_MESSAGE: unsafe extern "C" fn(
            receiver: *mut runtime::Value,
            message: *const runtime::Value,
        ) -> *mut runtime::Reply = runtime::send_message;

        module.get_function("send_message").unwrap_or_else(|| {
            module.add_function(
                "send_message",
                self.reply_ptr_type.fn_type(
                    &[self.value_ptr_type.into(), self.value_ptr_type.into()],
                    false,
                ),
                Some(Linkage::External),
            )
        })
    }

    fn poll_reply_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static POLL_REPLY: unsafe extern "C" fn(reply: *mut runtime::Reply) -> *mut runtime::Value =
            runtime::poll_reply;

        module.get_function("poll_reply").unwrap_or_else(|| {
            module.add_function(
                "poll_reply",
                self.value_ptr_type
                    .fn_type(&[self.reply_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn print_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[cfg(not(test))]
        #[used]
        static PRINT: unsafe extern "C" fn(value: *const runtime::Value) = runtime::print;

        module.get_function("print").unwrap_or_else(|| {
            module.add_function(
                "print",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }
}
