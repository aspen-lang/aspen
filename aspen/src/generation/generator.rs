use crate::generation::{EmittedModule, GenError, GenResult, Intrinsics};
use crate::semantics::{Host, Module as HostModule};
use crate::syntax;
use futures::executor::block_on;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::types::{FunctionType, IntType, PointerType, StructType, VoidType};
use inkwell::values::{BasicValue, FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use std::sync::Arc;

pub struct Generator<'ctx> {
    host: Host,
    context: &'ctx Context,

    pub void_type: VoidType<'ctx>,
    pub void_ptr_type: PointerType<'ctx>,
    pub bool_type: IntType<'ctx>,

    pub isize_type: IntType<'ctx>,
    pub i128_type: IntType<'ctx>,

    pub string_ptr_type: PointerType<'ctx>,

    pub object_ptr_type: StructType<'ctx>,
    pub opt0: PointerType<'ctx>,
    pub opt1: PointerType<'ctx>,
    pub opt2: PointerType<'ctx>,
    pub object_ptr_ref_type: PointerType<'ctx>,
    pub rt_ptr_type: PointerType<'ctx>,
    pub matcher_ptr_type: PointerType<'ctx>,

    pub start_fn_type: FunctionType<'ctx>,
    pub start_fn_ptr_type: PointerType<'ctx>,
    pub constructor_fn_type: FunctionType<'ctx>,
    pub init_fn_type: FunctionType<'ctx>,
    pub init_fn_ptr_type: PointerType<'ctx>,
    pub recv_fn_type: FunctionType<'ctx>,
    pub recv_fn_ptr_type: PointerType<'ctx>,
    pub drop_fn_type: FunctionType<'ctx>,
    pub drop_fn_ptr_type: PointerType<'ctx>,
    pub cont_fn_type: FunctionType<'ctx>,
    pub cont_fn_ptr_type: PointerType<'ctx>,
}

impl<'ctx> Generator<'ctx> {
    pub fn new(host: Host, context: &'ctx Context) -> Generator<'ctx> {
        let type_module = context.create_module("types");

        let void_type = context.void_type();

        let void_ptr_type = type_module
            .get_struct_type("Any")
            .unwrap_or_else(|| context.opaque_struct_type("Any"))
            .ptr_type(AddressSpace::Generic);

        let bool_type = context.bool_type();

        let i128_type = context.i128_type();

        #[cfg(target_pointer_width = "32")]
        let isize_type = context.i32_type();

        #[cfg(target_pointer_width = "64")]
        let isize_type = context.i64_type();

        let opt0 = type_module
            .get_struct_type("Object")
            .unwrap_or_else(|| context.opaque_struct_type("Object"))
            .ptr_type(AddressSpace::Generic);
        let opt1 = type_module
            .get_struct_type("RefCount")
            .unwrap_or_else(|| context.opaque_struct_type("RefCount"))
            .ptr_type(AddressSpace::Generic);

        let object_ptr_type = type_module.get_struct_type("ObjectRef").unwrap_or_else(|| {
            let object_ptr_type = context.opaque_struct_type("ObjectRef");
            object_ptr_type.set_body(&[opt0.into(), opt1.into()], false);
            object_ptr_type
        });

        let object_ptr_ref_type = object_ptr_type.ptr_type(AddressSpace::Generic);

        let opt2 = object_ptr_ref_type;

        let rt_ptr_type = type_module
            .get_struct_type("Runtime")
            .unwrap_or_else(|| context.opaque_struct_type("Runtime"))
            .ptr_type(AddressSpace::Generic);

        let matcher_ptr_type = type_module
            .get_struct_type("Matcher")
            .unwrap_or_else(|| context.opaque_struct_type("Matcher"))
            .ptr_type(AddressSpace::Generic);

        let string_ptr_type = context.i8_type().ptr_type(AddressSpace::Generic);

        let start_fn_type = void_type.fn_type(&[rt_ptr_type.into()], false);
        let start_fn_ptr_type = start_fn_type.ptr_type(AddressSpace::Generic);

        let init_fn_type = void_type.fn_type(
            &[
                rt_ptr_type.into(),
                object_ptr_ref_type.into(),
                void_ptr_type.into(),
                opt2.into(),
            ],
            false,
        );
        let init_fn_ptr_type = init_fn_type.ptr_type(AddressSpace::Generic);

        let recv_fn_type = void_type.fn_type(
            &[
                rt_ptr_type.into(),
                object_ptr_ref_type.into(),
                void_ptr_type.into(),
                opt0.into(),
                opt1.into(),
                opt2.into(),
            ],
            false,
        );
        let recv_fn_ptr_type = recv_fn_type.ptr_type(AddressSpace::Generic);

        let drop_fn_type = void_type.fn_type(&[rt_ptr_type.into(), void_ptr_type.into()], false);
        let drop_fn_ptr_type = drop_fn_type.ptr_type(AddressSpace::Generic);

        let cont_fn_type = void_type.fn_type(
            &[
                rt_ptr_type.into(),
                object_ptr_ref_type.into(),
                void_ptr_type.into(),
                void_ptr_type.into(),
                opt0.into(),
                opt1.into(),
                opt2.into(),
            ],
            false,
        );
        let cont_fn_ptr_type = cont_fn_type.ptr_type(AddressSpace::Generic);

        let constructor_fn_type =
            object_ptr_type.fn_type(&[rt_ptr_type.into(), opt0.into(), opt1.into()], false);

        Generator {
            host,
            context,

            void_type,
            void_ptr_type,
            bool_type,

            isize_type,
            i128_type,

            string_ptr_type,

            object_ptr_type,
            opt0,
            opt1,
            opt2,
            object_ptr_ref_type,
            rt_ptr_type,
            matcher_ptr_type,

            start_fn_type,
            start_fn_ptr_type,
            constructor_fn_type,
            init_fn_type,
            init_fn_ptr_type,
            recv_fn_type,
            recv_fn_ptr_type,
            drop_fn_type,
            drop_fn_ptr_type,
            cont_fn_type,
            cont_fn_ptr_type,
        }
    }

    pub fn generate_live_init(&self) -> GenResult<EmittedModule<'ctx>> {
        let module = self.context.create_module("live_init");
        let rt_global = module.add_global(self.rt_ptr_type, Some(AddressSpace::Generic), "RUNTIME");
        rt_global.set_initializer(&self.rt_ptr_type.const_zero());

        let init_fn = module.add_function("live_init", self.void_type.fn_type(&[], false), None);

        let intrinsics = Intrinsics::new(self, &module);

        let builder = self.context.create_builder();

        // Entry block of the Main function
        let entry_block = self.context.append_basic_block(init_fn, "entry");
        builder.position_at_end(entry_block);

        builder.build_store(
            rt_global.as_pointer_value(),
            intrinsics.new_runtime(&builder),
        );

        builder.build_return(None);

        Ok(EmittedModule::new_executable(module, intrinsics, init_fn))
    }

    pub fn generate_main<'a>(&'a self, main: &str) -> GenResult<EmittedModule<'ctx>> {
        let main = match block_on(self.host.find_declaration(main)) {
            None => return Err(GenError::InvalidMainObject(format!("`{}` is not defined", main))),
            Some(m) => m,
        };

        let main = match main.as_ref() {
            syntax::Declaration::Object(o) => o,
        };

        let module = self.context.create_module("main");

        let intrinsics = Intrinsics::new(self, &module);
        let builder = self.context.create_builder();

        let start_fn = module.add_function("start", self.start_fn_type, None);

        let main_fn = module.add_function("main", self.void_type.fn_type(&[], false), None);
        let entry_block = self.context.append_basic_block(main_fn, "entry");
        builder.position_at_end(entry_block);
        intrinsics.start_runtime(&builder, start_fn);
        builder.build_return(None);

        let entry_block = self.context.append_basic_block(start_fn, "entry");
        builder.position_at_end(entry_block);

        let main_object_constructor = module.add_function(
            ModuleGenerator::constructor_fn_name(&main).as_ref(),
            self.constructor_fn_type,
            Some(Linkage::External),
        );
        let main_object_ptr = builder.build_alloca(self.object_ptr_type, "main_object_ptr");
        let main_object = builder
            .build_call(
                main_object_constructor,
                &[start_fn.get_first_param().unwrap()],
                "main_object",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value();

        builder.build_store(main_object_ptr, main_object);
        intrinsics.tell(
            &builder,
            main_object_ptr,
            intrinsics.new_atom(&builder, "run!"),
        );
        intrinsics.drop(&builder, main_object);
        builder.build_return(None);

        Ok(EmittedModule::new_executable(module, intrinsics, main_fn))
    }

    pub fn generate_module<'a>(
        &'a self,
        module: &Arc<HostModule>,
    ) -> GenResult<EmittedModule<'ctx>> {
        let module_gen = self.create_module(module);

        match module_gen.generate_module()? {
            None => {
                Ok(EmittedModule::new(module_gen.module, module_gen.intrinsics))
            }
            Some(fun) => {
                Ok(EmittedModule::new_executable(
                    module_gen.module,
                    module_gen.intrinsics,
                    fun,
                ))
            }
        }
    }

    fn create_module<'mdl>(
        &'mdl self,
        host_module: &'mdl Arc<HostModule>,
    ) -> ModuleGenerator<'ctx, 'mdl> {
        let module = self.context.create_module(host_module.uri().as_ref());
        let intrinsics = Intrinsics::new(self, &module);

        ModuleGenerator {
            global: self,
            module,
            intrinsics,
            host_module,
        }
    }
}

struct ModuleGenerator<'ctx: 'mdl, 'mdl> {
    global: &'mdl Generator<'ctx>,
    module: Module<'ctx>,
    intrinsics: Intrinsics<'ctx>,
    host_module: &'mdl Arc<HostModule>,
}

impl<'ctx: 'mdl, 'mdl> ModuleGenerator<'ctx, 'mdl> {
    fn create_function<'fun>(
        &'fun self,
        name: &str,
        ty: FunctionType<'ctx>,
        linkage: Option<Linkage>,
    ) -> FunctionGenerator<'ctx, 'mdl, 'fun> {
        FunctionGenerator {
            module: self,
            function: self.module.get_function(name).unwrap_or_else(|| self.module.add_function(name, ty, linkage)),
            rt_reference: None,
            self_reference: None,
        }
    }

    fn generate_module(&self) -> GenResult<Option<FunctionValue<'ctx>>> {
        let root = self.host_module.syntax_tree();

        self.generate_root(root)
    }

    fn generate_root(&self, root: &Arc<syntax::Root>) -> GenResult<Option<FunctionValue<'ctx>>> {
        match root.as_ref() {
            syntax::Root::Module(syntax_module) => {
                self.generate_syntax_module(syntax_module)?;
                Ok(None)
            }
            syntax::Root::Inline(inline) => self.generate_inline(inline),
        }
    }

    fn generate_syntax_module(&self, syntax_module: &Arc<syntax::Module>) -> GenResult<()> {
        for d in syntax_module.declarations.iter() {
            self.generate_declaration(d)?;
        }
        Ok(())
    }

    fn generate_inline(
        &self,
        inline: &Arc<syntax::Inline>,
    ) -> GenResult<Option<FunctionValue<'ctx>>> {
        match inline.as_ref() {
            syntax::Inline::Declaration(d) => {
                self.generate_declaration(d)?;
                Ok(None)
            }
            syntax::Inline::Expression(e, _) => {
                let mut function =
                    self.create_function("Inline", self.global.void_type.fn_type(&[], false), None);

                let rt = self.module.add_global(
                    self.global.rt_ptr_type,
                    Some(AddressSpace::Generic),
                    "RUNTIME",
                );
                rt.set_externally_initialized(true);
                rt.set_linkage(Linkage::External);

                let builder = self.global.context.create_builder();
                let entry_block = function.append_block("entry");
                builder.position_at_end(entry_block);

                let rt = builder
                    .build_load(rt.as_pointer_value(), "")
                    .into_pointer_value();
                function.set_rt_reference(rt);

                let mut inline_recv =
                    self.create_function("Inline::Recv", self.global.recv_fn_type, None);
                inline_recv.with_rt_reference_in_first_parameter();
                inline_recv.with_self_reference_in_second_parameter();

                let actor = builder.build_alloca(self.global.object_ptr_type, "actor_ref");
                builder.build_store(
                    actor,
                    self.intrinsics
                        .new_stateless_actor(&builder, rt, inline_recv.function),
                );
                self.intrinsics
                    .tell(&builder, actor, self.intrinsics.new_atom(&builder, "run!"));
                builder.build_return(None);

                let entry_block = inline_recv.append_block("entry");
                builder.position_at_end(entry_block);
                if let Some(obj) =
                    inline_recv.generate_expression(&builder, e, ReplyHandling::Sync)?
                {
                    self.intrinsics.print(&builder, obj);
                }
                builder.build_return(None);

                Ok(Some(function.function))
            }
        }
    }

    fn generate_declaration(&self, declaration: &Arc<syntax::Declaration>) -> GenResult<()> {
        match declaration.as_ref() {
            syntax::Declaration::Object(o) => self.generate_object_declaration(o),
        }
    }

    fn constructor_fn_name(declaration: &Arc<syntax::ObjectDeclaration>) -> String {
        format!("{}::New", declaration.symbol())
    }

    fn init_fn_name(declaration: &Arc<syntax::ObjectDeclaration>) -> String {
        format!("{}::Init", declaration.symbol())
    }

    fn recv_fn_name(declaration: &Arc<syntax::ObjectDeclaration>) -> String {
        format!("{}::Recv", declaration.symbol())
    }

    fn drop_fn_name(declaration: &Arc<syntax::ObjectDeclaration>) -> String {
        format!("{}::Drop", declaration.symbol())
    }

    fn generate_object_declaration(
        &self,
        declaration: &Arc<syntax::ObjectDeclaration>,
    ) -> GenResult<()> {
        let constructor_fn_name = Self::constructor_fn_name(declaration);
        let init_fn_name = Self::init_fn_name(declaration);
        let recv_fn_name = Self::recv_fn_name(declaration);
        let drop_fn_name = Self::drop_fn_name(declaration);

        let init_fn = {
            let mut init_fn =
                self.create_function(init_fn_name.as_ref(), self.global.init_fn_type, None);
            init_fn.with_rt_reference_in_first_parameter();
            init_fn.with_self_reference_in_second_parameter();
            init_fn.generate_initializer(declaration)?;
            init_fn.function.as_global_value().as_pointer_value()
        };

        let recv_fn = {
            let mut recv_fn =
                self.create_function(recv_fn_name.as_ref(), self.global.recv_fn_type, None);
            recv_fn.with_rt_reference_in_first_parameter();
            recv_fn.with_self_reference_in_second_parameter();
            recv_fn.generate_receiver(declaration)?;
            recv_fn.function.as_global_value().as_pointer_value()
        };

        let drop_fn = {
            let mut drop_fn =
                self.create_function(drop_fn_name.as_ref(), self.global.drop_fn_type, None);
            drop_fn.with_rt_reference_in_first_parameter();
            drop_fn.generate_destructor(declaration)?;
            drop_fn.function.as_global_value().as_pointer_value()
        };

        {
            let mut constructor_fn = self.create_function(
                constructor_fn_name.as_ref(),
                self.global.constructor_fn_type,
                Some(Linkage::External),
            );
            constructor_fn.with_rt_reference_in_first_parameter();
            constructor_fn.generate_constructor(init_fn, recv_fn, drop_fn, declaration)?;
        }

        Ok(())
    }
}

struct FunctionGenerator<'ctx: 'mdl, 'mdl: 'fun, 'fun> {
    module: &'fun ModuleGenerator<'ctx, 'mdl>,
    function: FunctionValue<'ctx>,
    rt_reference: Option<PointerValue<'ctx>>,
    self_reference: Option<PointerValue<'ctx>>,
}

impl<'ctx: 'mdl, 'mdl: 'fun, 'fun> FunctionGenerator<'ctx, 'mdl, 'fun> {
    fn generate_expression<'cnt>(
        &mut self,
        builder: &Builder<'ctx>,
        expression: &Arc<syntax::Expression>,
        reply_handling: ReplyHandling,
    ) -> GenResult<Option<PointerValue<'ctx>>> {
        match expression.as_ref() {
            syntax::Expression::Integer(i) => Ok(Some(self.generate_integer(builder, i)?)),
            syntax::Expression::MessageSend(s) => {
                self.generate_message_send(builder, s, reply_handling)
            }
            syntax::Expression::Reference(r) => {
                Ok(Some(self.generate_reference_expression(builder, r)?))
            }
            syntax::Expression::Answer(a) => {
                self.generate_reply(builder, &a.expression, reply_handling)
            }
            _ => unimplemented!("expression {:?}", expression),
        }
    }

    fn object_ptr_param(
        &self,
        builder: &Builder<'ctx>,
        opt0: u32,
        name: &str,
    ) -> PointerValue<'ctx> {
        let first_component = self
            .function
            .get_nth_param(opt0)
            .unwrap()
            .into_pointer_value();

        if first_component.get_type() == self.module.global.object_ptr_ref_type {
            first_component
        } else {
            let object_ptr = builder.build_alloca(self.module.global.object_ptr_type, name);

            builder.build_store(
                builder.build_struct_gep(object_ptr, 0, "").unwrap(),
                self.function.get_nth_param(opt0).unwrap(),
            );
            builder.build_store(
                builder.build_struct_gep(object_ptr, 1, "").unwrap(),
                self.function.get_nth_param(opt0 + 1).unwrap(),
            );

            object_ptr
        }
    }

    fn generate_reply(
        &mut self,
        builder: &Builder<'ctx>,
        expression: &Arc<syntax::Expression>,
        reply_handling: ReplyHandling,
    ) -> GenResult<Option<PointerValue<'ctx>>> {
        let reply_to_ptr = self.object_ptr_param(builder, 3, "reply_to_ptr");

        let answer = self.generate_expression(builder, expression, ReplyHandling::Sync)?;

        match (answer, reply_handling) {
            (None, _) => Err(GenError::BadNode),

            (Some(answer), ReplyHandling::Async) => {
                let answer = builder.build_load(answer, "answer").into_struct_value();
                self.module.intrinsics.tell(builder, reply_to_ptr, answer);
                Ok(None)
            }

            (Some(answer), ReplyHandling::Sync) => {
                let answer_clone = self.module.intrinsics.clone(builder, answer);
                self.module
                    .intrinsics
                    .tell(builder, reply_to_ptr, answer_clone);
                Ok(Some(answer))
            }
        }
    }

    fn append_block(&self, name: &str) -> BasicBlock<'ctx> {
        self.module
            .global
            .context
            .append_basic_block(self.function, name)
    }

    fn set_self_reference(&mut self, self_ref: PointerValue<'ctx>) {
        self.self_reference = Some(self_ref);
    }

    fn with_self_reference_in_second_parameter(&mut self) {
        self.set_self_reference(self.function.get_nth_param(1).unwrap().into_pointer_value());
    }

    fn set_rt_reference(&mut self, rt_ref: PointerValue<'ctx>) {
        self.rt_reference = Some(rt_ref);
    }

    fn with_rt_reference_in_first_parameter(&mut self) {
        self.set_rt_reference(
            self.function
                .get_first_param()
                .unwrap()
                .into_pointer_value(),
        );
    }

    fn generate_message_send(
        &mut self,
        builder: &Builder<'ctx>,
        send: &Arc<syntax::MessageSend>,
        reply_handling: ReplyHandling,
    ) -> GenResult<Option<PointerValue<'ctx>>> {
        match self.generate_expression(builder, &send.receiver, ReplyHandling::Sync)? {
            None => Err(GenError::BadNode),
            Some(receiver) => {
                match self.generate_expression(builder, &send.message, ReplyHandling::Sync)? {
                    None => Err(GenError::BadNode),
                    Some(message) => {
                        let message = builder.build_load(message, "message").into_struct_value();
                        match reply_handling {
                            ReplyHandling::Sync => {
                                let cont_fn: FunctionGenerator<'ctx, 'mdl, 'fun> = self.create_continuation();
                                cont_fn.function.get_nth_param(0).unwrap().set_name("rt");
                                cont_fn.function.get_nth_param(1).unwrap().set_name("self");
                                cont_fn.function.get_nth_param(2).unwrap().set_name("state");
                                cont_fn.function.get_nth_param(3).unwrap().set_name("frame");
                                cont_fn
                                    .function
                                    .get_nth_param(4)
                                    .unwrap()
                                    .set_name("reply_to.0");
                                cont_fn
                                    .function
                                    .get_nth_param(5)
                                    .unwrap()
                                    .set_name("reply_to.1");
                                cont_fn
                                    .function
                                    .get_nth_param(6)
                                    .unwrap()
                                    .set_name("message");
                                cont_fn.function.add_attribute(
                                    inkwell::attributes::AttributeLoc::Param(6),
                                    self.module.global.context.create_enum_attribute(
                                        inkwell::attributes::Attribute::get_named_enum_kind_id(
                                            "byval",
                                        ),
                                        0,
                                    ),
                                );

                                let frame_type = self.module.global.context.opaque_struct_type(
                                    format!(
                                        "{}::Contd",
                                        self.function.get_name().to_str().unwrap()
                                    )
                                    .as_ref(),
                                );
                                frame_type.set_body(&[], false);
                                let frame_ptr_ptr = builder.build_alloca(
                                    self.module.global.void_ptr_type,
                                    "frame_ptr_ptr",
                                );

                                let drop_fn = self.module.module.add_function(
                                    format!(
                                        "{}::Drop",
                                        frame_type.get_name().unwrap().to_str().unwrap()
                                    )
                                    .as_ref(),
                                    self.module.global.drop_fn_type,
                                    None,
                                );
                                {
                                    let entry_block = self
                                        .module
                                        .global
                                        .context
                                        .append_basic_block(drop_fn, "entry");
                                    let builder = self.module.global.context.create_builder();
                                    builder.position_at_end(entry_block);
                                    builder.build_return(None);
                                }

                                let continuation = self.module.intrinsics.continuation(
                                    builder,
                                    self.rt_reference.unwrap(),
                                    self.self_reference.unwrap(),
                                    frame_type.size_of().unwrap(),
                                    frame_ptr_ptr,
                                    cont_fn.function,
                                    drop_fn,
                                );

                                let frame_ptr_ptr = builder.build_bitcast(
                                    frame_ptr_ptr,
                                    frame_type.ptr_type(AddressSpace::Generic),
                                    "",
                                );
                                let _frame_ptr = builder
                                    .build_load(frame_ptr_ptr.into_pointer_value(), "frame_ptr");

                                self.module.intrinsics.ask(
                                    builder,
                                    receiver,
                                    continuation.into(),
                                    message.into(),
                                );

                                builder.build_return(None);

                                let entry_block = cont_fn.append_block("entry");
                                builder.position_at_end(entry_block);

                                let message_ptr =
                                    cont_fn.object_ptr_param(builder, 6, "message_ptr");

                                let _ = std::mem::replace(self, cont_fn);

                                Ok(Some(message_ptr))
                            }
                            ReplyHandling::Async => {
                                self.module.intrinsics.tell(builder, receiver, message);
                                Ok(None)
                            }
                        }
                    }
                }
            }
        }
    }

    fn create_continuation(&self) -> FunctionGenerator<'ctx, 'mdl, 'fun> {
        let mut gen = FunctionGenerator {
            module: &self.module,
            function: self.module.module.add_function(
                self.function.get_name().to_str().unwrap(),
                self.module.global.cont_fn_type,
                None,
            ),
            rt_reference: None,
            self_reference: None,
        };
        gen.with_rt_reference_in_first_parameter();
        gen.with_self_reference_in_second_parameter();
        gen
    }

    fn generate_integer(
        &self,
        builder: &Builder<'ctx>,
        int: &Arc<syntax::Integer>,
    ) -> GenResult<PointerValue<'ctx>> {
        let int_ptr = builder.build_alloca(self.module.global.object_ptr_type, "int_ptr");
        builder.build_store(
            int_ptr,
            self.module
                .intrinsics
                .new_int(builder, self.generate_integer_literal(int)?.into()),
        );
        Ok(int_ptr)
    }

    fn generate_integer_literal(&self, int: &Arc<syntax::Integer>) -> GenResult<IntValue<'ctx>> {
        if let syntax::TokenKind::IntegerLiteral(value, _) = int.literal.kind {
            Ok(self.module.global.i128_type.const_int_arbitrary_precision(
                [value as u64, value.wrapping_shr(64) as u64].as_ref(),
            ))
        } else {
            Err(GenError::BadNode)
        }
    }

    fn generate_reference_expression(
        &self,
        builder: &Builder<'ctx>,
        expression: &Arc<syntax::ReferenceExpression>,
    ) -> GenResult<PointerValue<'ctx>> {
        let declaration = block_on(
            self.module
                .host_module
                .declaration_referenced_by(expression.clone()),
        );
        if declaration.is_none() {
            return Err(GenError::BadNode);
        }
        let declaration = declaration.unwrap();

        match declaration.as_ref() {
            syntax::Declaration::Object(o) => {
                let constructor_fn_name = ModuleGenerator::constructor_fn_name(o);
                let constructor = self.module.module.add_function(
                    constructor_fn_name.as_ref(),
                    self.module.global.constructor_fn_type,
                    Some(Linkage::External),
                );
                let object_ptr =
                    builder.build_alloca(self.module.global.object_ptr_type, "object_ptr");
                let (opt0, opt1) = self
                    .module
                    .intrinsics
                    .split_object_ptr(builder, self.module.intrinsics.new_atom(builder, "new!"));
                let object = builder
                    .build_call(
                        constructor,
                        &[
                            self.rt_reference
                                .expect("cannot instantiate object without a runtime in scope")
                                .into(),
                            opt0,
                            opt1,
                        ],
                        "object",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                builder.build_store(object_ptr, object);
                Ok(object_ptr)
            }
        }
    }

    fn generate_constructor(
        &self,
        init_fn: PointerValue<'ctx>,
        recv_fn: PointerValue<'ctx>,
        drop_fn: PointerValue<'ctx>,
        _declaration: &Arc<syntax::ObjectDeclaration>,
    ) -> GenResult<()> {
        let builder = self.module.global.context.create_builder();
        let entry_block = self.append_block("entry");
        builder.position_at_end(entry_block);

        let init_msg = builder
            .build_load(self.object_ptr_param(&builder, 1, ""), "init_msg")
            .into_struct_value();
        let object = self.module.intrinsics.new_actor(
            &builder,
            self.rt_reference
                .expect("cannot instantiate object without a runtime in scope"),
            self.module.global.isize_type.const_int(32, false),
            init_msg,
            init_fn,
            recv_fn,
            drop_fn,
        );
        builder.build_return(Some(&object));
        Ok(())
    }

    fn generate_destructor(&self, _declaration: &Arc<syntax::ObjectDeclaration>) -> GenResult<()> {
        let builder = self.module.global.context.create_builder();
        let entry_block = self.append_block("entry");
        builder.position_at_end(entry_block);
        builder.build_return(None);
        Ok(())
    }

    fn generate_receiver(&mut self, declaration: &Arc<syntax::ObjectDeclaration>) -> GenResult<()> {
        self.function.get_nth_param(0).unwrap().set_name("rt");
        self.function.get_nth_param(1).unwrap().set_name("self");
        self.function.get_nth_param(2).unwrap().set_name("state");
        self.function
            .get_nth_param(3)
            .unwrap()
            .set_name("reply_to.0");
        self.function
            .get_nth_param(4)
            .unwrap()
            .set_name("reply_to.1");
        self.function.get_nth_param(5).unwrap().set_name("message");
        self.function.add_attribute(
            inkwell::attributes::AttributeLoc::Param(5),
            self.module.global.context.create_enum_attribute(
                inkwell::attributes::Attribute::get_named_enum_kind_id("byval"),
                0,
            ),
        );

        let builder = self.module.global.context.create_builder();
        let exit_block = self.append_block("exit");
        builder.position_at_end(exit_block);
        builder.build_return(None);

        let entry_block = self.append_block("entry");
        builder.position_at_end(entry_block);

        for method in declaration.methods() {
            self.generate_method(&builder, method, exit_block)?;
        }
        builder.build_unconditional_branch(exit_block);
        exit_block
            .move_after(builder.get_insert_block().unwrap())
            .unwrap();
        Ok(())
    }

    fn generate_method(
        &mut self,
        builder: &Builder<'ctx>,
        method: &Arc<syntax::Method>,
        exit_block: BasicBlock<'ctx>,
    ) -> GenResult<()> {
        let matcher = self.generate_pattern_matcher(builder, &method.pattern)?;
        let message_ptr = self.object_ptr_param(builder, 5, "message_ptr");

        let match_block = self.append_block(format!("{:?}", method.pattern).as_ref());
        let else_block = self.append_block("else");

        builder.build_conditional_branch(
            self.module
                .intrinsics
                .match_obj(builder, matcher, message_ptr),
            match_block,
            else_block,
        );

        builder.position_at_end(match_block);
        self.module.intrinsics.drop_matcher(builder, matcher);

        for statement in method.statements.iter() {
            self.generate_statement(builder, statement)?;
        }

        builder.build_unconditional_branch(exit_block);

        builder.position_at_end(else_block);
        self.module.intrinsics.drop_matcher(builder, matcher);
        if let Some(_) = self.function.get_nth_param(3) {
            let reply_to_ptr = self.object_ptr_param(builder, 3, "reply_to_ptr");
            self.module.intrinsics.tell(
                builder,
                reply_to_ptr,
                self.module
                    .intrinsics
                    .new_atom(builder, "didNotUnderstand!"),
            );
        }

        Ok(())
    }

    fn generate_statement(
        &mut self,
        builder: &Builder<'ctx>,
        statement: &Arc<syntax::Statement>,
    ) -> GenResult<()> {
        self.generate_expression(builder, &statement.expression, ReplyHandling::Async)?;
        Ok(())
    }

    fn generate_pattern_matcher(
        &self,
        builder: &Builder<'ctx>,
        pattern: &Arc<syntax::Pattern>,
    ) -> GenResult<PointerValue<'ctx>> {
        match pattern.as_ref() {
            syntax::Pattern::Integer(i) => Ok(self
                .module
                .intrinsics
                .eq_int(builder, self.generate_integer_literal(i)?)),
            p => unimplemented!("pattern matcher for pattern {:?}", p),
        }
    }

    fn generate_initializer(&self, _declaration: &Arc<syntax::ObjectDeclaration>) -> GenResult<()> {
        let builder = self.module.global.context.create_builder();
        let entry_block = self.append_block("entry");
        builder.position_at_end(entry_block);
        builder.build_return(None);
        Ok(())
    }
}

enum ReplyHandling {
    Sync,
    Async,
}
