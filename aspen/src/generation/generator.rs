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
use inkwell::types::{
    BasicType, FloatType, FunctionType, IntType, PointerType, StructType, VoidType,
};
use inkwell::values::{BasicValueEnum, FunctionValue, PointerValue};
use inkwell::AddressSpace;
use std::fmt;
use std::sync::Arc;

pub struct Generator<'ctx> {
    context: &'ctx Context,
    #[allow(unused)]
    host: Host,

    // void
    void_type: VoidType<'ctx>,
    // *i8
    str_type: PointerType<'ctx>,

    // i128
    i128_type: IntType<'ctx>,
    // f64
    f64_type: FloatType<'ctx>,
    // usize
    usize_type: IntType<'ctx>,

    // u32
    tag_type: IntType<'ctx>,

    // { tag: ValueTag::Object, ref_count: *usize, ptr: *Object }
    object_ref_type: StructType<'ctx>,
    // { tag: ValueTag::Integer, ref_count: *usize, value: i128 }
    integer_type: StructType<'ctx>,
    // { tag: ValueTag::Float, ref_count: *usize, value: f64 }
    float_type: StructType<'ctx>,
    // *{ tag: usize, ref_count: *usize, ... }
    value_ptr_type: PointerType<'ctx>,

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

        let i128_type = context.i128_type();
        let f64_type = context.f64_type();
        let usize_type = context.custom_width_int_type((std::mem::size_of::<usize>() * 8) as u32);

        let object_type = context.opaque_struct_type("Object");
        let object_ptr_type = object_type.ptr_type(AddressSpace::Generic);

        let tag_type = usize_type;

        let object_ref_type = context.opaque_struct_type("Object");
        object_ref_type.set_body(
            &[
                tag_type.into(),
                usize_type.ptr_type(AddressSpace::Generic).into(),
                object_ptr_type.into(),
            ],
            false,
        );

        let integer_type = context.opaque_struct_type("Integer");
        integer_type.set_body(
            &[
                tag_type.into(),
                usize_type.ptr_type(AddressSpace::Generic).into(),
                i128_type.into(),
            ],
            false,
        );

        let float_type = context.opaque_struct_type("Float");
        float_type.set_body(
            &[
                tag_type.into(),
                usize_type.ptr_type(AddressSpace::Generic).into(),
                f64_type.into(),
            ],
            false,
        );

        let value_type = context.opaque_struct_type("Value");
        value_type.set_body(
            &[
                tag_type.into(),
                usize_type.ptr_type(AddressSpace::Generic).into(),
            ],
            false,
        );

        let value_ptr_type = value_type.ptr_type(AddressSpace::Generic);

        let void_fn_type = void_type.fn_type(&[], false);
        let main_fn_type = i32_type.fn_type(&[], false);

        Generator {
            context,
            host,
            void_type,
            str_type,
            usize_type,
            i128_type,
            f64_type,
            tag_type,
            object_ref_type,
            integer_type,
            float_type,
            value_ptr_type,
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

        let (value_ptr, object_ptr) = self.generate_object_box(&builder, main_type);
        builder.build_call(main_init_fn, &[object_ptr.into()], "");

        let print_fn = self.print_fn(&module);
        let drop_reference_fn = self.drop_reference_fn(&module);

        builder.build_call(print_fn, &[value_ptr.into()], "");
        builder.build_call(drop_reference_fn, &[value_ptr.into()], "");

        let status_code = context.i32_type().const_int(13, false);
        builder.build_return(Some(&status_code));

        Ok(EmittedModule {
            module,
            init_fn: Some(main_fn),
        })
    }

    fn print_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[repr(C)]
        struct Value {
            _private: [u8; 0],
        }

        #[cfg(not(test))]
        #[link(name = "aspen_runtime")]
        extern "C" {
            fn print(value: *const Value);
        }
        {
            #[cfg(not(test))]
            #[used]
            static USED: unsafe extern "C" fn(*const Value) = print;
        }

        module.get_function("print").unwrap_or_else(|| {
            module.add_function(
                "print",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
                Some(Linkage::External),
            )
        })
    }

    fn drop_reference_fn(&self, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        #[repr(C)]
        struct Value {
            _private: [u8; 0],
        }

        #[cfg(not(test))]
        #[link(name = "aspen_runtime")]
        extern "C" {
            fn drop_reference(value: *mut Value);
        }
        {
            #[cfg(not(test))]
            #[used]
            static USED: unsafe extern "C" fn(*mut Value) = drop_reference;
        }

        module.get_function("drop_reference").unwrap_or_else(|| {
            module.add_function(
                "drop_reference",
                self.void_type.fn_type(&[self.value_ptr_type.into()], false),
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
            syntax::Inline::Expression(expression, _) => {
                let builder = self.context.create_builder();

                let run_fn =
                    module.add_function("run_inline", self.void_fn_type, Some(Linkage::External));
                {
                    let entry_block = self.context.append_basic_block(run_fn, "entry");
                    builder.position_at_end(entry_block);

                    let object =
                        self.generate_expression(host_module, module, &builder, expression)?;

                    let print_fn = self.print_fn(module);
                    let drop_reference_fn = self.drop_reference_fn(module);

                    builder.build_call(print_fn, &[object], "");
                    builder.build_call(drop_reference_fn, &[object], "");

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
            syntax::Expression::Integer(i) => self.generate_integer(builder, i),
            syntax::Expression::Float(f) => self.generate_float(builder, f),
        }
    }

    fn generate_integer(
        &self,
        builder: &Builder<'ctx>,
        integer: &Arc<syntax::Integer>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let ptr = self.generate_value(builder, 0xf1, self.integer_type);
        let value = builder.build_struct_gep(ptr, 2, "value").unwrap();

        let n = match &integer.literal.kind {
            syntax::TokenKind::IntegerLiteral(n, _) => *n,
            _ => return Err(GenError::BadNode),
        };

        let words = [n as u64, n.wrapping_shr(64) as u64];
        builder.build_store(
            value,
            self.i128_type.const_int_arbitrary_precision(words.as_ref()),
        );

        Ok(builder
            .build_pointer_cast(ptr, self.value_ptr_type, "value")
            .into())
    }

    fn generate_value(
        &self,
        builder: &Builder<'ctx>,
        tag: usize,
        type_: impl BasicType<'ctx>,
    ) -> PointerValue<'ctx> {
        // Allocate the boxed value
        let ptr = builder.build_malloc(type_, "ptr").unwrap();

        {
            let tag_ptr = builder.build_struct_gep(ptr, 0, "tag").unwrap();

            // Store the tag in the box
            builder.build_store(tag_ptr, self.tag_type.const_int(tag as u64, false));
        }

        {
            let ref_count_ptr_ptr = builder.build_struct_gep(ptr, 1, "ref_count").unwrap();

            // Allocate the reference counter
            let ref_count_ptr = builder.build_malloc(self.usize_type, "ref_count").unwrap();

            // Initialize the reference counter with 1
            builder.build_store(ref_count_ptr, self.usize_type.const_int(1, false));
            builder.build_store(ref_count_ptr_ptr, ref_count_ptr);
        }

        ptr
    }

    fn generate_float(
        &self,
        builder: &Builder<'ctx>,
        float: &Arc<syntax::Float>,
    ) -> GenResult<BasicValueEnum<'ctx>> {
        let ptr = self.generate_value(builder, 0xf2, self.float_type);
        let value = builder.build_struct_gep(ptr, 2, "value").unwrap();

        let n = match &float.literal.kind {
            syntax::TokenKind::FloatLiteral(n, _) => *n,
            _ => return Err(GenError::BadNode),
        };
        builder.build_store(value, self.f64_type.const_float(n));

        Ok(builder
            .build_pointer_cast(ptr, self.value_ptr_type, "value")
            .into())
    }

    fn generate_object_box(
        &self,
        builder: &Builder<'ctx>,
        object_type: StructType<'ctx>,
    ) -> (PointerValue<'ctx>, PointerValue<'ctx>) {
        let object_ptr = builder.build_malloc(object_type, "object").unwrap();
        let object_box_ptr = self.generate_value(builder, 0xf0, self.object_ref_type);

        // Pointer to the pointer slot within the object ref box
        let object_box_ptr_ptr = builder.build_struct_gep(object_box_ptr, 2, "ptr").unwrap();
        builder.build_store(object_box_ptr_ptr, object_ptr);

        (
            builder
                .build_bitcast(object_box_ptr, self.value_ptr_type, "")
                .into_pointer_value(),
            object_ptr,
        )
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

                let (value_ptr, object_ptr) = self.generate_object_box(builder, type_);
                builder.build_call(new_fn, &[object_ptr.into()], "");

                Ok(value_ptr.into())
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

            let _object = init_fn.get_first_param().unwrap();
            // TODO: Initialize all fields on object

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

            let _object = to_string_fn.get_first_param().unwrap();
            // TODO: Recursively call the ToString method for each field on the object

            let as_string = builder.build_global_string_ptr(qn, "as_string");

            builder.build_return(Some(&as_string));
        }
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
