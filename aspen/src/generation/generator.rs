use crate::generation::{GenError, GenResult};
use crate::semantics::types::Type;
use crate::semantics::{Host, Module as HostModule};
use crate::syntax;
use aspen_runtime;
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
    // bool
    bool_type: IntType<'ctx>,

    // *i8
    str_ptr_type: PointerType<'ctx>,
    // *Value
    value_ptr_type: PointerType<'ctx>,
    // *Slot
    slot_ptr_type: PointerType<'ctx>,
    // *PendingReply
    pending_reply_ptr_type: PointerType<'ctx>,

    // () -> void
    void_fn_type: FunctionType<'ctx>,
    // () -> i32
    main_fn_type: FunctionType<'ctx>,
    // (*T, *Value) -> *Value
    recv_fn_type: FunctionType<'ctx>,
    // *(() -> void)
    recv_fn_ptr_type: PointerType<'ctx>,
}

impl<'ctx> Generator<'ctx> {
    pub fn new(host: Host, context: &'ctx Context) -> Generator<'ctx> {
        let void_type = context.void_type();
        let i32_type = context.i32_type();
        let usize_type = context.custom_width_int_type((std::mem::size_of::<usize>() * 8) as u32);

        let i8_type = context.i8_type();
        let i128_type = context.i128_type();
        let f64_type = context.f64_type();
        let bool_type = context.bool_type();
        let str_ptr_type = i8_type.ptr_type(AddressSpace::Generic);

        let pending_reply_type = context.opaque_struct_type("PendingReply");
        let pending_reply_ptr_type = pending_reply_type.ptr_type(AddressSpace::Generic);

        let value_type = context.opaque_struct_type("Value");
        let value_ptr_type = value_type.ptr_type(AddressSpace::Generic);

        let slot_type = context.opaque_struct_type("Slot");
        let slot_ptr_type = slot_type.ptr_type(AddressSpace::Generic);

        let void_fn_type = void_type.fn_type(&[], false);
        let main_fn_type = i32_type.fn_type(&[], false);

        let recv_fn_type = void_type.fn_type(
            &[
                context
                    .opaque_struct_type("Object")
                    .ptr_type(AddressSpace::Generic)
                    .into(),
                value_ptr_type.into(),
                slot_ptr_type.into(),
            ],
            false,
        );
        let recv_fn_ptr_type = recv_fn_type.ptr_type(AddressSpace::Generic);

        Generator {
            context,
            host,
            void_type,
            usize_type,
            i128_type,
            f64_type,
            bool_type,
            str_ptr_type,
            value_ptr_type,
            slot_ptr_type,
            pending_reply_ptr_type,
            void_fn_type,
            main_fn_type,
            recv_fn_type,
            recv_fn_ptr_type,
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

        let new_object_fn = self.new_object_fn(&module);
        let print_fn = self.print_fn(&module);
        let main_object_recv = module
            .add_function(
                format!("{}::recv", main).as_str(),
                self.recv_fn_type,
                Some(Linkage::External),
            )
            .as_global_value()
            .as_pointer_value();
        let main_object_state_size = self.usize_type.const_int(0, false);
        let main_obj = builder
            .build_call(
                new_object_fn,
                &[main_object_state_size.into(), main_object_recv.into()],
                "main_obj",
            )
            .try_as_basic_value()
            .left()
            .unwrap();

        let run_atom = builder
            .build_call(
                self.new_nullary_fn(&module),
                &[builder
                    .build_global_string_ptr("run!", "run_atom_name")
                    .as_pointer_value()
                    .into()],
                "run_atom",
            )
            .try_as_basic_value()
            .left()
            .unwrap();

        let mut locals = vec![main_obj, run_atom];

        let result = self.generate_message_send_impl(
            &module,
            main_fn,
            &builder,
            &mut locals,
            main_obj,
            run_atom,
        )?;

        builder.build_call(print_fn, &[result.into()], "");

        self.generate_end_of_scope(&module, &builder, locals);

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

                    let type_ = block_on(host_module.get_type_of(expression.clone()));
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

                    let type_ = format!(":: {}", type_);
                    let type_ = builder.build_global_string_ptr(type_.as_ref(), "");
                    let type_ = builder.build_call(
                        self.new_string_fn(module),
                        &[type_.as_pointer_value().into()],
                        "",
                    );
                    builder.build_call(print_fn, &[type_.try_as_basic_value().left().unwrap()], "");

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
            syntax::Expression::NullaryAtom(i) => {
                self.generate_nullary(module, builder, locals, i.atom.lexeme())
            }
            syntax::Expression::Answer(a) => {
                self.generate_answer(host_module, module, function, builder, locals, a)
            }
        }
    }

    fn generate_answer(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        function: FunctionValue<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        answer: &Arc<syntax::AnswerExpression>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let value = self.generate_expression(
            host_module,
            module,
            function,
            builder,
            locals,
            &answer.expression,
        )?;

        let slot = function.get_nth_param(2).unwrap();
        builder.build_call(self.clone_reference_fn(module), &[value], "");
        builder.build_call(self.answer_fn(module), &[slot, value], "");
        Ok(value)
    }

    fn generate_nullary(
        &self,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        atom: &str,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let atom_value = builder
            .build_call(
                self.new_nullary_fn(module),
                &[builder
                    .build_global_string_ptr(atom, "")
                    .as_pointer_value()
                    .into()],
                "",
            )
            .try_as_basic_value()
            .left()
            .unwrap();

        locals.push(atom_value);

        Ok(atom_value)
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

        self.generate_message_send_impl(module, function, builder, locals, receiver, message)
    }

    fn generate_message_send_impl(
        &self,
        module: &Module<'ctx>,
        function: FunctionValue<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        receiver: BasicValueEnum<'ctx>,
        message: BasicValueEnum<'ctx>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let send_message_fn = self.send_message_fn(module);
        let clone_reference_fn = self.clone_reference_fn(module);
        let poll_reply = self.poll_reply_fn(module);

        builder.build_call(clone_reference_fn, &[message], "");
        builder.build_call(clone_reference_fn, &[receiver], "");

        let pending_reply = builder
            .build_call(send_message_fn, &[receiver, message], "pending_reply")
            .try_as_basic_value()
            .left()
            .unwrap();

        let poll_loop_block = self.context.append_basic_block(function, "poll_loop");

        builder.build_unconditional_branch(poll_loop_block);
        builder.position_at_end(poll_loop_block);

        let object = builder
            .build_call(poll_reply, &[pending_reply], "")
            .try_as_basic_value()
            .left()
            .unwrap();

        let reply_int = builder.build_ptr_to_int(object.into_pointer_value(), self.usize_type, "");

        let if_reply_is_null = builder.build_int_compare(
            IntPredicate::EQ,
            reply_int,
            self.usize_type.const_zero(),
            "if_reply_is_null",
        );

        let exit_block = self
            .context
            .insert_basic_block_after(poll_loop_block, "exit");

        builder.build_conditional_branch(if_reply_is_null, poll_loop_block, exit_block);

        builder.position_at_end(exit_block);

        // TODO: Have sender panic when recipient panics
        let _if_reply_is_panic = builder.build_int_compare(
            IntPredicate::EQ,
            reply_int,
            self.usize_type.const_int('P' as u64, false),
            "if_reply_is_panic",
        );

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
            Type::Object(o) => {
                let state_size = self.usize_type.const_zero();
                let recv = self
                    .object_recv_fn(module, &o)
                    .as_global_value()
                    .as_pointer_value();

                let object_ref = builder
                    .build_call(
                        self.new_object_fn(module),
                        &[state_size.into(), recv.into()],
                        "",
                    )
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
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        declaration: &Arc<syntax::ObjectDeclaration>,
    ) -> GenResult<()> {
        let drop_reference_fn = self.drop_reference_fn(module);

        let recv_fn = self.object_recv_fn(module, declaration);
        let builder = self.context.create_builder();

        let entry_block = self.context.append_basic_block(recv_fn, "entry");
        builder.position_at_end(entry_block);

        let _state = recv_fn.get_nth_param(0).unwrap();
        let message = recv_fn.get_nth_param(1).unwrap();
        let slot = recv_fn.get_nth_param(2).unwrap();

        for method in declaration.methods() {
            self.generate_method(
                host_module,
                module,
                recv_fn,
                &builder,
                message,
                slot,
                method,
            )?;
        }

        builder.build_call(drop_reference_fn, &[message], "");
        builder.build_return(Some(&self.value_ptr_type.const_zero()));

        Ok(())
    }

    fn generate_method(
        &self,
        host_module: &Arc<HostModule>,
        module: &Module<'ctx>,
        function: FunctionValue<'ctx>,
        builder: &Builder<'ctx>,
        message: BasicValueEnum<'ctx>,
        _slot: BasicValueEnum<'ctx>,
        method: &Arc<syntax::Method>,
    ) -> GenResult<()> {
        let mut locals = vec![message];

        let builder = self.generate_pattern_match(
            module,
            function,
            builder,
            &mut locals,
            &method.pattern,
            message,
        )?;

        // let ok = self.generate_nullary(module, &builder, &mut locals, "ok!")?;
        // builder.build_call(self.answer_fn(module), &[slot, ok], "");

        for statement in method.statements.iter() {
            Some(Box::new(self.generate_expression(
                host_module,
                module,
                function,
                &builder,
                &mut locals,
                &statement.expression,
            )?));
        }

        self.generate_end_of_scope(module, &builder, locals);
        builder.build_return(None);
        Ok(())
    }

    fn generate_pattern_match(
        &self,
        module: &Module<'ctx>,
        function: FunctionValue<'ctx>,
        builder: &Builder<'ctx>,
        locals: &mut Vec<BasicValueEnum<'ctx>>,
        pattern: &Arc<syntax::Pattern>,
        subject: BasicValueEnum<'ctx>,
    ) -> GenResult<Builder<'ctx>> {
        let match_fn = self.match_fn(module);

        let matches = match pattern.as_ref() {
            syntax::Pattern::Integer(i) => {
                let pattern = self.generate_integer(module, builder, locals, i)?;
                builder
                    .build_call(match_fn, &[pattern, subject], "matches")
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value()
            }
            syntax::Pattern::Nullary(a) => {
                let pattern = self.generate_nullary(module, builder, locals, a.atom.lexeme())?;
                builder
                    .build_call(match_fn, &[pattern, subject], "matches")
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value()
            }
        };

        let match_block = self
            .context
            .append_basic_block(function, format!("if_matches={:?}", pattern).as_str());
        let else_block = self.context.append_basic_block(function, "else");

        builder.build_conditional_branch(matches, match_block, else_block);
        builder.position_at_end(else_block);

        let builder = self.context.create_builder();
        builder.position_at_end(match_block);

        Ok(builder)
    }

    fn object_recv_fn(
        &self,
        module: &Module<'ctx>,
        declaration: &Arc<syntax::ObjectDeclaration>,
    ) -> FunctionValue<'ctx> {
        let name = format!("{}::recv", declaration.symbol());

        module.get_function(name.as_ref()).unwrap_or_else(|| {
            module.add_function(name.as_ref(), self.recv_fn_type, Some(Linkage::External))
        })
    }
}

pub struct EmittedModule<'ctx> {
    pub module: Module<'ctx>,
    init_fn: Option<FunctionValue<'ctx>>,
}

impl<'ctx> EmittedModule<'ctx> {
    pub unsafe fn evaluate(&self, generator: &Generator<'ctx>, engine: ExecutionEngine<'ctx>) {
        engine.add_module(&self.module).unwrap_or(());

        generator.map_runtime_in_jit(&self.module, &engine);

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

impl<'ctx> Generator<'ctx> {
    pub fn map_runtime_in_jit(&self, module: &Module<'ctx>, engine: &ExecutionEngine<'ctx>) {
        engine.add_global_mapping(&self.new_int_fn(module), aspen_runtime::new_int as usize);
        engine.add_global_mapping(
            &self.new_float_fn(module),
            aspen_runtime::new_float as usize,
        );
        engine.add_global_mapping(
            &self.new_string_fn(module),
            aspen_runtime::new_string as usize,
        );
        engine.add_global_mapping(
            &self.new_object_fn(module),
            aspen_runtime::new_object as usize,
        );
        engine.add_global_mapping(
            &self.new_nullary_fn(module),
            aspen_runtime::new_nullary as usize,
        );

        engine.add_global_mapping(&self.match_fn(module), aspen_runtime::r#match as usize);

        engine.add_global_mapping(
            &self.clone_reference_fn(module),
            aspen_runtime::clone_reference as usize,
        );
        engine.add_global_mapping(
            &self.drop_reference_fn(module),
            aspen_runtime::drop_reference as usize,
        );

        engine.add_global_mapping(
            &self.send_message_fn(module),
            aspen_runtime::send_message as usize,
        );
        engine.add_global_mapping(
            &self.poll_reply_fn(module),
            aspen_runtime::poll_reply as usize,
        );
        engine.add_global_mapping(&self.answer_fn(module), aspen_runtime::answer as usize);

        engine.add_global_mapping(&self.print_fn(module), aspen_runtime::print as usize);
    }

    fn new_int_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("new_int").unwrap_or_else(|| {
            module.add_function(
                "new_int",
                self.value_ptr_type.fn_type(&[self.i128_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn new_float_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("new_float").unwrap_or_else(|| {
            module.add_function(
                "new_float",
                self.value_ptr_type.fn_type(&[self.f64_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn new_string_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("new_string").unwrap_or_else(|| {
            module.add_function(
                "new_string",
                self.value_ptr_type
                    .fn_type(&[self.str_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn new_object_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("new_object").unwrap_or_else(|| {
            module.add_function(
                "new_object",
                self.value_ptr_type.fn_type(
                    &[self.usize_type.into(), self.recv_fn_ptr_type.into()],
                    false,
                ),
                Some(Linkage::External),
            )
        })
    }

    fn new_nullary_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("new_nullary").unwrap_or_else(|| {
            module.add_function(
                "new_nullary",
                self.value_ptr_type
                    .fn_type(&[self.str_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn match_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("match").unwrap_or_else(|| {
            module.add_function(
                "match",
                self.bool_type.fn_type(
                    &[self.value_ptr_type.into(), self.value_ptr_type.into()],
                    false,
                ),
                Some(Linkage::External),
            )
        })
    }

    fn clone_reference_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("clone_reference").unwrap_or_else(|| {
            module.add_function(
                "clone_reference",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn drop_reference_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("drop_reference").unwrap_or_else(|| {
            module.add_function(
                "drop_reference",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn send_message_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("send_message").unwrap_or_else(|| {
            module.add_function(
                "send_message",
                self.pending_reply_ptr_type.fn_type(
                    &[self.value_ptr_type.into(), self.value_ptr_type.into()],
                    false,
                ),
                Some(Linkage::External),
            )
        })
    }

    fn poll_reply_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("poll_reply").unwrap_or_else(|| {
            module.add_function(
                "poll_reply",
                self.value_ptr_type
                    .fn_type(&[self.pending_reply_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn answer_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("answer").unwrap_or_else(|| {
            module.add_function(
                "answer",
                self.void_type.fn_type(
                    &[self.value_ptr_type.into(), self.slot_ptr_type.into()],
                    false,
                ),
                Some(Linkage::External),
            )
        })
    }

    fn print_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        module.get_function("print").unwrap_or_else(|| {
            module.add_function(
                "print",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }
}
