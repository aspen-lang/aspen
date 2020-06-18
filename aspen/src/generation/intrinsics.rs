use crate::generation::Generator;
use inkwell::builder::Builder;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Linkage;
use inkwell::module::Module;
use inkwell::values::{
    BasicValue, BasicValueEnum, FunctionValue, IntValue, PointerValue, StructValue,
};
use inkwell::AddressSpace;

#[allow(non_snake_case)]
pub struct Intrinsics<'ctx> {
    AspenNewRuntime: FunctionValue<'ctx>,
    AspenStartRuntime: FunctionValue<'ctx>,
    AspenPrint: FunctionValue<'ctx>,
    AspenNewInt: FunctionValue<'ctx>,
    AspenNewAtom: FunctionValue<'ctx>,
    AspenClone: FunctionValue<'ctx>,
    AspenDrop: FunctionValue<'ctx>,
    AspenTell: FunctionValue<'ctx>,
    AspenAsk: FunctionValue<'ctx>,
    AspenNewActor: FunctionValue<'ctx>,
    AspenNewStatelessActor: FunctionValue<'ctx>,
    AspenEqInt: FunctionValue<'ctx>,
    AspenMatch: FunctionValue<'ctx>,
    AspenDropMatcher: FunctionValue<'ctx>,
    AspenContinue: FunctionValue<'ctx>,
}

impl<'ctx> Intrinsics<'ctx> {
    pub fn new(generator: &Generator<'ctx>, module: &Module<'ctx>) -> Intrinsics<'ctx> {
        macro_rules! signature {
            ($($name:ident ($($param:expr $(,)?)*) -> $return_type:expr)*) => {
                Intrinsics {
                    $(
                        $name: module.add_function(
                            stringify!($name),
                            $return_type.fn_type(&[
                                $($param.into(),)*
                            ], false),
                            Some(Linkage::External),
                        ),
                    )*
                }
            }
        }

        signature! {
            AspenNewRuntime() -> generator.rt_ptr_type
            AspenStartRuntime(generator.start_fn_ptr_type) -> generator.void_type
            AspenPrint(generator.object_ptr_ref_type) -> generator.void_type
            AspenNewInt(generator.i128_type) -> generator.object_ptr_type
            AspenNewAtom(generator.string_ptr_type) -> generator.object_ptr_type
            AspenClone(generator.object_ptr_ref_type) -> generator.object_ptr_type
            AspenDrop(generator.opt0, generator.opt1) -> generator.void_type
            AspenTell(
                generator.object_ptr_ref_type,
                generator.opt0, generator.opt1,
            ) -> generator.void_type
            AspenAsk(
                generator.object_ptr_ref_type,
                generator.opt0, generator.opt1,
                generator.opt0, generator.opt1,
            ) -> generator.void_type
            AspenNewActor(
                generator.rt_ptr_type,
                generator.isize_type,
                generator.opt0, generator.opt1,
                generator.init_fn_ptr_type,
                generator.recv_fn_ptr_type,
                generator.drop_fn_ptr_type,
            ) -> generator.object_ptr_type
            AspenNewStatelessActor(
                generator.rt_ptr_type,
                generator.recv_fn_ptr_type,
            ) -> generator.object_ptr_type
            AspenEqInt(generator.i128_type) -> generator.matcher_ptr_type
            AspenMatch(generator.matcher_ptr_type, generator.object_ptr_ref_type) -> generator.bool_type
            AspenDropMatcher(generator.matcher_ptr_type) -> generator.void_type
            AspenContinue(
                generator.rt_ptr_type,
                generator.object_ptr_ref_type,
                generator.isize_type,
                generator.void_ptr_type.ptr_type(AddressSpace::Generic),
                generator.cont_fn_ptr_type,
                generator.drop_fn_ptr_type,
            ) -> generator.object_ptr_type
        }
    }

    pub fn map_in_jit(&self, engine: &ExecutionEngine<'ctx>) {
        macro_rules! map {
            ($($name:ident)*) => {
                $(
                    engine.add_global_mapping(&self.$name, aspenrt::$name as usize);
                )*
            }
        }

        map! {
            AspenNewRuntime
            AspenStartRuntime
            AspenPrint
            AspenNewInt
            AspenNewAtom
            AspenClone
            AspenDrop
            AspenTell
            AspenAsk
            AspenNewActor
            AspenNewStatelessActor
            AspenEqInt
            AspenMatch
            AspenDropMatcher
            AspenContinue
        }
    }

    pub fn new_runtime(&self, builder: &Builder<'ctx>) -> PointerValue<'ctx> {
        builder
            .build_call(self.AspenNewRuntime, &[], "rt")
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value()
    }

    pub fn start_runtime(&self, builder: &Builder<'ctx>, start_fn: FunctionValue<'ctx>) {
        builder.build_call(
            self.AspenStartRuntime,
            &[start_fn.as_global_value().as_pointer_value().into()],
            "",
        );
    }

    pub fn new_int(&self, builder: &Builder<'ctx>, int: IntValue<'ctx>) -> StructValue<'ctx> {
        builder
            .build_call(self.AspenNewInt, &[int.into()], "new_int")
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    pub fn new_atom(&self, builder: &Builder<'ctx>, name: &str) -> StructValue<'ctx> {
        builder
            .build_call(
                self.AspenNewAtom,
                &[builder
                    .build_global_string_ptr(name, name)
                    .as_pointer_value()
                    .into()],
                "new_atom",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    pub fn print(&self, builder: &Builder<'ctx>, object: PointerValue<'ctx>) {
        builder.build_call(self.AspenPrint, &[object.into()], "");
    }

    pub fn clone(&self, builder: &Builder<'ctx>, object: PointerValue<'ctx>) -> StructValue<'ctx> {
        builder
            .build_call(self.AspenClone, &[object.into()], "")
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    pub fn drop(&self, builder: &Builder<'ctx>, object: StructValue<'ctx>) {
        let (opt0, opt1) = self.split_object_ptr(builder, object);
        builder.build_call(self.AspenDrop, &[opt0, opt1], "");
    }

    pub fn split_object_ptr(
        &self,
        builder: &Builder<'ctx>,
        object: StructValue<'ctx>,
    ) -> (BasicValueEnum<'ctx>, BasicValueEnum<'ctx>) {
        let ptr = builder.build_alloca(object.get_type(), "");
        builder.build_store(ptr, object);
        (
            builder
                .build_load(builder.build_struct_gep(ptr, 0, "").unwrap(), "")
                .into(),
            builder
                .build_load(builder.build_struct_gep(ptr, 1, "").unwrap(), "")
                .into(),
        )
    }

    pub fn tell(
        &self,
        builder: &Builder<'ctx>,
        receiver: PointerValue<'ctx>,
        message: StructValue<'ctx>,
    ) {
        let (opt0, opt1) = self.split_object_ptr(builder, message);
        builder.build_call(self.AspenTell, &[receiver.into(), opt0, opt1], "");
    }

    pub fn ask(
        &self,
        builder: &Builder<'ctx>,
        receiver: PointerValue<'ctx>,
        reply_to: StructValue<'ctx>,
        message: StructValue<'ctx>,
    ) {
        let (opt0, opt1) = self.split_object_ptr(builder, reply_to);
        let (opt2, opt3) = self.split_object_ptr(builder, message);

        builder.build_call(
            self.AspenAsk,
            &[receiver.into(), opt0, opt1, opt2, opt3],
            "",
        );
    }

    pub fn new_actor(
        &self,
        builder: &Builder<'ctx>,
        rt: PointerValue<'ctx>,
        state_size: IntValue<'ctx>,
        init_msg: StructValue<'ctx>,
        init_fn: PointerValue<'ctx>,
        recv_fn: PointerValue<'ctx>,
        drop_fn: PointerValue<'ctx>,
    ) -> StructValue<'ctx> {
        let (opt0, opt1) = self.split_object_ptr(builder, init_msg);

        builder
            .build_call(
                self.AspenNewActor,
                &[
                    rt.into(),
                    state_size.into(),
                    opt0,
                    opt1,
                    init_fn.into(),
                    recv_fn.into(),
                    drop_fn.into(),
                ],
                "actor",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    pub fn new_stateless_actor(
        &self,
        builder: &Builder<'ctx>,
        rt: PointerValue<'ctx>,
        recv_fn: FunctionValue<'ctx>,
    ) -> StructValue<'ctx> {
        builder
            .build_call(
                self.AspenNewStatelessActor,
                &[
                    rt.into(),
                    recv_fn.as_global_value().as_pointer_value().into(),
                ],
                "actor",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }

    pub fn eq_int<I: BasicValue<'ctx>>(
        &self,
        builder: &Builder<'ctx>,
        int: I,
    ) -> PointerValue<'ctx> {
        builder
            .build_call(self.AspenEqInt, &[int.as_basic_value_enum()], "eq_int")
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value()
    }

    pub fn match_obj(
        &self,
        builder: &Builder<'ctx>,
        matcher: PointerValue<'ctx>,
        obj: PointerValue<'ctx>,
    ) -> IntValue<'ctx> {
        builder
            .build_call(self.AspenMatch, &[matcher.into(), obj.into()], "matches")
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_int_value()
    }

    pub fn drop_matcher(&self, builder: &Builder<'ctx>, matcher: PointerValue<'ctx>) {
        builder.build_call(self.AspenDropMatcher, &[matcher.into()], "");
    }

    pub fn continuation(
        &self,
        builder: &Builder<'ctx>,
        rt: PointerValue<'ctx>,
        self_: PointerValue<'ctx>,
        continuation_frame_size: IntValue<'ctx>,
        continuation_frame_ptr: PointerValue<'ctx>,
        continuation_fn: FunctionValue<'ctx>,
        drop_fn: FunctionValue<'ctx>,
    ) -> StructValue<'ctx> {
        builder
            .build_call(
                self.AspenContinue,
                &[
                    rt.into(),
                    self_.into(),
                    continuation_frame_size.into(),
                    continuation_frame_ptr.into(),
                    continuation_fn.as_global_value().as_pointer_value().into(),
                    drop_fn.as_global_value().as_pointer_value().into(),
                ],
                "continuation",
            )
            .try_as_basic_value()
            .left()
            .unwrap()
            .into_struct_value()
    }
}
