//! Backend-independent, explicitly sequenced IR. Binding IDs are lexical identities,
//! never evidence pointers or interface method positions.

use crate::types::{
    AdaptationPlan, BindingTemplate, BoundAdaptation, PatternPlan, Type, TypedExprKind,
    TypedExpression, TypedMethod, TypedStatement,
};
use crate::{Selector, Span};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrProgram {
    pub entry: Block,
    pub globals: Block,
    pub actors: Vec<ActorDefinition>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Block {
    pub instructions: Vec<Instruction>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub result: Option<ValueId>,
    pub span: Span,
    pub operation: Operation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendMode {
    WaitFirst,
    NoReply,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Startup's runtime-provided syscall capability, unavailable to source lookup.
    Syscall,
    Global(String),
    DefineGlobal {
        name: String,
        value: ValueId,
    },
    Int(i64),
    Float(crate::FloatValue),
    String(String),
    Read(BindingId),
    Selector(Selector<ValueId>),
    /// Every execution creates a fresh actor identity, even with no captures.
    CreateActor {
        definition: ActorId,
        captures: Vec<(BindingId, ValueId)>,
    },
    Bind {
        binding: BindingId,
        value: ValueId,
        path: ProjectionPath,
    },
    /// The full message is dispatched by its runtime shape, not a static slot.
    Send {
        callee: ValueId,
        message: ValueId,
        mode: SendMode,
        adaptation: Option<AdaptationPlan>,
        bound_adaptations: Vec<BoundAdaptation>,
        unreachable: bool,
    },
    /// Static evidence only: this does not wrap, copy, or replace the value.
    Check {
        value: ValueId,
        adaptation: AdaptationPlan,
    },
}
/// Payload ordinals at each nested selector; atomic selectors have no payload.
pub type ProjectionPath = Vec<usize>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputBinding {
    pub binding: BindingId,
    pub name: String,
    pub path: ProjectionPath,
    pub span: Span,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorDefinition {
    pub id: ActorId,
    pub span: Span,
    pub captures: Vec<BindingId>,
    pub globals: Vec<String>,
    pub methods: Vec<Method>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    pub span: Span,
    /// Runtime dispatch may erase this accepted input to its shape.
    pub input: Type,
    pub dispatch: crate::dispatch::RuntimeShape,
    pub message: ValueId,
    pub inputs: Vec<InputBinding>,
    pub reply_parameter: Option<BindingId>,
    pub body: Block,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LowerError {
    MissingBinding { name: String, span: Span },
    DispatchOverlap { first: Span, second: Span },
    UnsupportedAdaptation { span: Span },
}
impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBinding { name, span } => write!(
                f,
                "no runtime binding for {name:?} at {}:{}",
                span.start.line, span.start.col
            ),
            Self::DispatchOverlap { first, second } => write!(
                f,
                "overlapping runtime receiver shapes at {}:{} and {}:{}",
                first.start.line, first.start.col, second.start.line, second.start.col
            ),
            Self::UnsupportedAdaptation { span } => write!(
                f,
                "non-identity runtime adaptation at {}:{} is not supported",
                span.start.line, span.start.col
            ),
        }
    }
}
impl std::error::Error for LowerError {}

#[derive(Clone, Default)]
struct Scope {
    names: BTreeMap<String, BindingId>,
    reply: Option<BindingId>,
}
#[derive(Default)]
struct Lowerer {
    next_value: u32,
    next_binding: u32,
    next_actor: u32,
    actors: Vec<ActorDefinition>,
    globals: BTreeSet<String>,
}
/// Lower a closed, checked program. Open expressions require runtime bindings
/// and produce a located error rather than guessing identities from provenance.
pub fn lower_program(statements: &[TypedStatement]) -> Result<IrProgram, LowerError> {
    let mut lowerer = Lowerer::default();
    let entry = lowerer.statements(statements, &mut Scope::default())?;
    lowerer.actors.sort_by_key(|actor| actor.id);
    let program = IrProgram {
        entry,
        globals: Block::default(),
        actors: lowerer.actors,
    };
    program.validate()?;
    Ok(program)
}
/// Initialize the checked static graph once, then execute its entry send.
pub fn lower_globals(
    globals: &[(String, TypedExpression)],
    entry: &TypedStatement,
) -> Result<IrProgram, LowerError> {
    let mut lowerer = Lowerer {
        globals: globals.iter().map(|(name, _)| name.clone()).collect(),
        ..Lowerer::default()
    };
    let mut initialization = Block::default();
    for (name, expression) in globals {
        let value = lowerer.expression(expression, &Scope::default(), &mut initialization)?;
        Lowerer::effect(
            &mut initialization,
            expression.evidence.expression,
            Operation::DefineGlobal {
                name: name.clone(),
                value,
            },
        );
    }
    let entry = lowerer.statements(std::slice::from_ref(entry), &mut Scope::default())?;
    lowerer.actors.sort_by_key(|actor| actor.id);
    let program = IrProgram {
        entry,
        globals: initialization,
        actors: lowerer.actors,
    };
    program.validate()?;
    Ok(program)
}
impl Lowerer {
    fn value(&mut self) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        id
    }
    fn binding(&mut self) -> BindingId {
        let id = BindingId(self.next_binding);
        self.next_binding += 1;
        id
    }
    fn emit(&mut self, block: &mut Block, span: Span, operation: Operation) -> ValueId {
        let result = self.value();
        block.instructions.push(Instruction {
            result: Some(result),
            span,
            operation,
        });
        result
    }
    fn effect(block: &mut Block, span: Span, operation: Operation) {
        block.instructions.push(Instruction {
            result: None,
            span,
            operation,
        });
    }
    fn pattern(
        &mut self,
        plan: &PatternPlan,
        path: &mut ProjectionPath,
        scope: &mut Scope,
        output: &mut Vec<InputBinding>,
    ) {
        match &plan.template {
            BindingTemplate::Discard => {}
            BindingTemplate::Variable { name, origin, .. } => {
                let binding = self.binding();
                scope.names.insert(name.clone(), binding);
                output.push(InputBinding {
                    binding,
                    name: name.clone(),
                    path: path.clone(),
                    span: *origin,
                });
            }
            BindingTemplate::Selector(children) => {
                for (index, child) in children.values().into_iter().enumerate() {
                    path.push(index);
                    self.pattern(child, path, scope, output);
                    path.pop();
                }
            }
        }
    }
    fn statements(
        &mut self,
        statements: &[TypedStatement],
        scope: &mut Scope,
    ) -> Result<Block, LowerError> {
        let mut block = Block::default();
        for statement in statements {
            match statement {
                TypedStatement::Expr(value) => {
                    self.expression(value, scope, &mut block)?;
                }
                TypedStatement::Let(checked) => {
                    let value = self.expression(&checked.value, scope, &mut block)?;
                    Self::effect(
                        &mut block,
                        checked.plan.expectation.origin,
                        Operation::Check {
                            value,
                            adaptation: checked.adaptation.clone(),
                        },
                    );
                    let mut bindings = Vec::new();
                    self.pattern(&checked.plan, &mut Vec::new(), scope, &mut bindings);
                    for binding in bindings {
                        Self::effect(
                            &mut block,
                            binding.span,
                            Operation::Bind {
                                binding: binding.binding,
                                value,
                                path: binding.path,
                            },
                        );
                    }
                }
                TypedStatement::NoReplySend {
                    span,
                    callee,
                    message,
                    method,
                    adaptation,
                    bound_adaptations,
                } => {
                    self.send(
                        callee,
                        message,
                        (
                            Some(*method),
                            Some(adaptation.clone()),
                            bound_adaptations.clone(),
                            SendMode::NoReply,
                        ),
                        *span,
                        scope,
                        &mut block,
                    )?;
                }
            }
        }
        Ok(block)
    }
    fn send(
        &mut self,
        callee: &TypedExpression,
        message: &TypedExpression,
        selection: (
            Option<usize>,
            Option<AdaptationPlan>,
            Vec<BoundAdaptation>,
            SendMode,
        ),
        span: Span,
        scope: &Scope,
        block: &mut Block,
    ) -> Result<Option<ValueId>, LowerError> {
        let (method, adaptation, bound_adaptations, mode) = selection;
        // Evaluation order is observable: callee, then the complete payload, then send.
        let callee_value = self.expression(callee, scope, block)?;
        let message_value = self.expression(message, scope, block)?;
        let operation = Operation::Send {
            callee: callee_value,
            message: message_value,
            mode,
            adaptation,
            bound_adaptations,
            unreachable: method.is_none(),
        };
        Ok(match mode {
            SendMode::WaitFirst => Some(self.emit(block, span, operation)),
            SendMode::NoReply => {
                Self::effect(block, span, operation);
                None
            }
        })
    }
    fn expression(
        &mut self,
        expression: &TypedExpression,
        scope: &Scope,
        block: &mut Block,
    ) -> Result<ValueId, LowerError> {
        let span = expression.evidence.expression;
        let value = match &expression.kind {
            TypedExprKind::Syscall => self.emit(block, span, Operation::Syscall),
            TypedExprKind::Int(value) => self.emit(block, span, Operation::Int(*value)),
            TypedExprKind::Float(value) => self.emit(block, span, Operation::Float(*value)),
            TypedExprKind::String(value) => {
                self.emit(block, span, Operation::String(value.clone()))
            }
            TypedExprKind::Variable { binding } => {
                if let Some(id) = scope.names.get(&binding.name).copied() {
                    self.emit(block, span, Operation::Read(id))
                } else if self.globals.contains(&binding.name) {
                    self.emit(block, span, Operation::Global(binding.name.clone()))
                } else {
                    return Err(LowerError::MissingBinding {
                        name: binding.name.clone(),
                        span,
                    });
                }
            }
            TypedExprKind::ReplyTo { .. } => {
                let id = scope.reply.ok_or_else(|| LowerError::MissingBinding {
                    name: "^".into(),
                    span,
                })?;
                self.emit(block, span, Operation::Read(id))
            }
            TypedExprKind::Selector(selector) => {
                let mut error = None;
                let values = selector.map(|child| match self.expression(child, scope, block) {
                    Ok(value) => value,
                    Err(e) => {
                        error = Some(e);
                        ValueId(0)
                    }
                });
                if let Some(error) = error {
                    return Err(error);
                }
                self.emit(block, span, Operation::Selector(values))
            }
            TypedExprKind::Send {
                callee,
                message,
                method,
                adaptation,
                bound_adaptations,
            } => self
                .send(
                    callee,
                    message,
                    (
                        *method,
                        adaptation.clone(),
                        bound_adaptations.clone(),
                        SendMode::WaitFirst,
                    ),
                    span,
                    scope,
                    block,
                )?
                .expect("wait-first send has a result"),
            TypedExprKind::Actor { methods } => {
                if let Type::Actor(actor) = &expression.evidence.ty {
                    crate::dispatch::validate_actor_inputs(actor).map_err(|overlap| {
                        LowerError::DispatchOverlap {
                            first: methods[overlap.first].span,
                            second: methods[overlap.second].span,
                        }
                    })?;
                }
                let id = ActorId(self.next_actor);
                self.next_actor += 1;
                let methods = methods
                    .iter()
                    .map(|method| self.method(method, scope))
                    .collect::<Result<Vec<_>, _>>()?;
                let outer = scope
                    .names
                    .values()
                    .copied()
                    .chain(scope.reply)
                    .collect::<BTreeSet<_>>();
                let mut captured = BTreeSet::new();
                for method in &methods {
                    for instruction in &method.body.instructions {
                        if let Operation::Read(binding) = instruction.operation
                            && outer.contains(&binding)
                        {
                            captured.insert(binding);
                        }
                    }
                }
                let captures = captured.into_iter().collect::<Vec<_>>();
                let values = captures
                    .iter()
                    .map(|binding| (*binding, self.emit(block, span, Operation::Read(*binding))))
                    .collect();
                let mut globals = BTreeSet::new();
                for method in &methods {
                    for instruction in &method.body.instructions {
                        match &instruction.operation {
                            Operation::Global(name) => {
                                globals.insert(name.clone());
                            }
                            Operation::CreateActor { definition, .. } => {
                                let nested =
                                    self.actors.iter().find(|a| a.id == *definition).unwrap();
                                globals.extend(nested.globals.iter().cloned());
                            }
                            _ => {}
                        }
                    }
                }
                self.actors.push(ActorDefinition {
                    id,
                    span,
                    captures,
                    globals: globals.into_iter().collect(),
                    methods,
                });
                self.emit(
                    block,
                    span,
                    Operation::CreateActor {
                        definition: id,
                        captures: values,
                    },
                )
            }
        };
        for adaptation in &expression.adaptations {
            Self::effect(
                block,
                span,
                Operation::Check {
                    value,
                    adaptation: adaptation.clone(),
                },
            );
        }
        Ok(value)
    }
    fn method(&mut self, method: &TypedMethod, outer: &Scope) -> Result<Method, LowerError> {
        let mut scope = outer.clone();
        // Direct ^ always belongs to this method; ordinary aliases remain lexical.
        scope.reply = method.reply.as_ref().map(|_| self.binding());
        let reply_parameter = scope.reply;
        let message = self.value();
        let mut inputs = Vec::new();
        self.pattern(&method.plan, &mut Vec::new(), &mut scope, &mut inputs);
        let body = self.statements(&method.body, &mut scope)?;
        let signature = crate::types::MethodType {
            parameters: method
                .parameters
                .iter()
                .map(|p| p.parameter.clone())
                .collect(),
            input: method.expectation.ty.clone(),
            reply: method.reply.as_ref().map(|r| r.ty.clone()),
        };
        let input = signature.accepted_input();
        Ok(Method {
            span: method.span,
            dispatch: crate::dispatch::RuntimeShape::from_type(&input),
            input,
            message,
            inputs,
            reply_parameter,
            body,
        })
    }
}

impl IrProgram {
    /// Check the identity-only backend contract before handing IR to codegen.
    pub fn validate(&self) -> Result<(), LowerError> {
        for block in std::iter::once(&self.entry)
            .chain(std::iter::once(&self.globals))
            .chain(
                self.actors
                    .iter()
                    .flat_map(|a| a.methods.iter().map(|m| &m.body)),
            )
        {
            for instruction in &block.instructions {
                let identity = match &instruction.operation {
                    Operation::Check { adaptation, .. } => adaptation.is_identity(),
                    Operation::Send {
                        adaptation,
                        bound_adaptations,
                        ..
                    } => {
                        adaptation.as_ref().is_none_or(AdaptationPlan::is_identity)
                            && bound_adaptations.iter().all(|a| a.plan.is_identity())
                    }
                    _ => true,
                };
                if !identity {
                    return Err(LowerError::UnsupportedAdaptation {
                        span: instruction.span,
                    });
                }
            }
        }
        for actor in &self.actors {
            for (index, method) in actor.methods.iter().enumerate() {
                for previous in &actor.methods[..index] {
                    if !method.dispatch.is_disjoint_from(&previous.dispatch) {
                        return Err(LowerError::DispatchOverlap {
                            first: previous.span,
                            second: method.span,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lexer, parse, types::check_program};

    fn compile(source: &str) -> IrProgram {
        let mut diagnostics = Vec::new();
        let parsed = parse(Lexer::new(source), &mut diagnostics);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        lower_program(&check_program(&parsed).unwrap()).unwrap()
    }

    #[test]
    fn global_references_are_edges_not_lexical_captures() {
        let mut diagnostics = Vec::new();
        let parsed = parse(
            Lexer::new("let target = {}. { def go => { def nested => target. }. }. target."),
            &mut diagnostics,
        );
        assert!(diagnostics.is_empty());
        let checked = check_program(&parsed).unwrap();
        let TypedStatement::Let(target) = &checked[0] else {
            panic!()
        };
        let TypedStatement::Expr(actor) = &checked[1] else {
            panic!()
        };
        let ir = lower_globals(
            &[
                ("target".into(), target.value.clone()),
                ("actor".into(), actor.clone()),
            ],
            &checked[2],
        )
        .unwrap();
        assert!(ir.actors.iter().all(|a| a.captures.is_empty()));
        assert_eq!(ir.actors[1].globals, ["target"]);
        assert_eq!(ir.actors[2].globals, ["target"]);
        assert!(
            matches!(&ir.entry.instructions[0].operation, Operation::Global(name) if name == "target")
        );
        assert_eq!(
            ir.globals
                .instructions
                .iter()
                .filter(|i| matches!(i.operation, Operation::DefineGlobal { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn nested_pattern_paths_and_initializer_shadowing() {
        let ir = compile(
            "let x = {}. let #outer: (#inner: x) = #outer: (#inner: x). x. { def outer: (#inner: y) => y. }.",
        );
        let bindings = ir
            .entry
            .instructions
            .iter()
            .filter_map(|i| match &i.operation {
                Operation::Bind { binding, path, .. } => Some((*binding, path.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(bindings[0].1, Vec::<usize>::new());
        assert_eq!(bindings[1].1, [0, 0]);
        assert_ne!(bindings[0].0, bindings[1].0);
        let reads = ir
            .entry
            .instructions
            .iter()
            .filter_map(|i| match i.operation {
                Operation::Read(id) => Some(id),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(reads, [bindings[0].0, bindings[1].0]);
        assert_eq!(ir.actors[1].methods[0].inputs[0].path, [0, 0]);
    }

    #[test]
    fn reply_alias_and_transitive_capture_are_explicit() {
        let ir = compile(
            "{ def go -> #ok => let target = ^. { def middle => { def inner => target (#ok). }. }. }.",
        );
        let outer = &ir.actors[0].methods[0];
        let reply = outer.reply_parameter.unwrap();
        assert!(matches!(outer.body.instructions[0].operation, Operation::Read(id) if id == reply));
        let Operation::Bind { binding: alias, .. } = outer.body.instructions[2].operation else {
            panic!()
        };
        assert_eq!(ir.actors[1].captures, [alias]);
        assert_eq!(ir.actors[2].captures, [alias]);
        assert!(ir.actors[1].methods[0].reply_parameter.is_none());
        let send = ir.actors[2].methods[0].body.instructions.last().unwrap();
        assert!(matches!(
            send.operation,
            Operation::Send {
                mode: SendMode::NoReply,
                ..
            }
        ));
        assert!(send.result.is_none());
    }

    #[test]
    fn sends_sequence_callee_then_payload_left_to_right() {
        let ir = compile(
            "({ def make -> { ({}) } => } make) (#left: ({ def one -> {} => } one) right: ({ def two -> {} => } two)).",
        );
        let sends = ir
            .entry
            .instructions
            .iter()
            .filter(|i| matches!(i.operation, Operation::Send { .. }))
            .collect::<Vec<_>>();
        assert_eq!(sends.len(), 4);
        assert!(sends[0].span.start.col < sends[1].span.start.col);
        assert!(sends[1].span.start.col < sends[2].span.start.col);
        let Operation::Send {
            callee,
            message,
            mode: SendMode::NoReply,
            ..
        } = sends[3].operation
        else {
            panic!()
        };
        assert_eq!(Some(callee), sends[0].result);
        let selector = ir
            .entry
            .instructions
            .iter()
            .find(|i| i.result == Some(message))
            .unwrap();
        let Operation::Selector(Selector::Keyword(payloads)) = &selector.operation else {
            panic!()
        };
        assert_eq!(Some(payloads[0].1), sends[1].result);
        assert_eq!(Some(payloads[1].1), sends[2].result);
    }

    #[test]
    fn bound_plans_survive_without_becoming_dispatch_slots() {
        let ir =
            compile("{ def use: ({ start. stop } a) => a start. a stop. } use: { def (x) => }.");
        let Operation::Send {
            bound_adaptations, ..
        } = &ir.entry.instructions.last().unwrap().operation
        else {
            panic!()
        };
        assert_eq!(bound_adaptations.len(), 1);
        assert!(matches!(
            bound_adaptations[0].plan,
            AdaptationPlan::Actor { .. }
        ));
        assert!(bound_adaptations[0].plan.is_identity());
        assert_eq!(
            ir.actors[0].methods[0].dispatch,
            crate::dispatch::RuntimeShape::Selector(Selector::Keyword(vec![(
                "use".into(),
                crate::dispatch::RuntimeShape::Actor
            )]))
        );
    }

    #[test]
    fn checked_expression_and_binding_plans_keep_precision() {
        use crate::types::{Expectation, Type, TypedStatement, check};
        let mut diagnostics = Vec::new();
        let mut parsed = parse(Lexer::new("{}."), &mut diagnostics);
        let crate::Stmt::Expr(expression) = parsed.statements.remove(0).value else {
            panic!()
        };
        let checked = check(
            &expression,
            &Expectation {
                ty: Type::UNIT,
                origin: expression.span,
            },
        )
        .unwrap();
        assert_eq!(checked.evidence.ty, Type::UNIT);
        assert_eq!(checked.adaptations.len(), 1);
        let ir = lower_program(&[TypedStatement::Expr(checked)]).unwrap();
        assert!(matches!(
            ir.entry.instructions[1].operation,
            Operation::Check { .. }
        ));
        let mut ir = compile("{ def left => def right => }.");
        ir.actors[0].methods[1].dispatch = ir.actors[0].methods[0].dispatch.clone();
        assert!(matches!(
            ir.validate(),
            Err(LowerError::DispatchOverlap { .. })
        ));
    }

    #[test]
    fn actors_are_fresh_and_method_results_never_implicitly_reply() {
        let ir = compile(
            "{}. {}. { def empty -> #ok => def value -> {} => {}. def bottom: (never x) => x go. }.",
        );
        assert_eq!(ir.entry.instructions.len(), 3);
        assert_eq!(ir.actors.len(), 4);
        let methods = &ir.actors[2].methods;
        assert!(methods[0].body.instructions.is_empty());
        assert!(matches!(
            methods[1].body.instructions[0].operation,
            Operation::CreateActor { .. }
        ));
        assert!(methods[2].dispatch.is_empty());
        assert!(matches!(
            methods[2].body.instructions.last().unwrap().operation,
            Operation::Send {
                unreachable: true,
                ..
            }
        ));
    }
}
