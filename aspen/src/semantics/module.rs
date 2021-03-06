use crate::semantics::types::{Behaviour, Type};
use crate::semantics::*;
use crate::syntax::*;
use crate::{Diagnostics, Source, SourceKind, URI};
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

pub struct Module {
    pub source: Arc<Source>,
    root_node: Arc<Root>,
    diagnostics: Mutex<Diagnostics>,
    pub host: Host,

    // Analyzers
    exported_declarations: MemoOut<analyzers::GetExportedDeclarations>,
    collect_diagnostics: Once<
        MergeTwo<
            MergeTwo<
                MergeTwo<
                    MergeTwo<
                        analyzers::CheckForDuplicateExports,
                        analyzers::CheckAllReferencesAreDefined,
                    >,
                    analyzers::CheckForFailedExpressionTypeInference,
                >,
                analyzers::CheckForFailedTypeExpressionTypeInference,
            >,
            analyzers::CheckForUnunderstandableMessages,
        >,
    >,
    find_declaration: Memo<analyzers::FindDeclaration, usize>,
    find_type_declaration: Memo<analyzers::FindTypeDeclaration, usize>,
    get_type_of_expression: Memo<analyzers::GetTypeOfExpression, usize>,
    get_type_of_type_expression: Memo<analyzers::GetTypeOfTypeExpression, usize>,
    get_behaviours_of_object: Memo<analyzers::GetBehavioursOfObject, usize>,
}

impl Module {
    pub async fn parse(source: Arc<Source>, host: Host) -> Module {
        let (root_node, diagnostics) = Parser::new(source.clone()).parse().await;

        Module {
            source,
            root_node,
            diagnostics: Mutex::new(diagnostics),
            host,

            exported_declarations: MemoOut::of(analyzers::GetExportedDeclarations),
            collect_diagnostics: Once::of(
                (analyzers::CheckForDuplicateExports)
                    .and(analyzers::CheckAllReferencesAreDefined)
                    .and(analyzers::CheckForFailedExpressionTypeInference)
                    .and(analyzers::CheckForFailedTypeExpressionTypeInference)
                    .and(analyzers::CheckForUnunderstandableMessages),
            ),
            find_declaration: Memo::of(analyzers::FindDeclaration),
            find_type_declaration: Memo::of(analyzers::FindTypeDeclaration),
            get_type_of_expression: Memo::of(analyzers::GetTypeOfExpression),
            get_type_of_type_expression: Memo::of(analyzers::GetTypeOfTypeExpression),
            get_behaviours_of_object: Memo::of(analyzers::GetBehavioursOfObject),
        }
    }

    pub fn uri(&self) -> &URI {
        self.source.uri()
    }

    pub fn kind(&self) -> &SourceKind {
        &self.source.kind
    }

    pub fn syntax_tree(&self) -> &Arc<Root> {
        &self.root_node
    }

    pub fn modified(&self) -> &SystemTime {
        &self.source.modified
    }

    pub fn navigate(&self) -> Arc<Navigator> {
        Navigator::new(self.root_node.clone())
    }

    async fn run_analyzer<A: Analyzer>(
        self: &Arc<Self>,
        analyzer: &A,
        input: A::Input,
    ) -> A::Output {
        let ctx = AnalysisContext {
            input,
            module: self.clone(),
            host: self.host.clone(),
            navigator: Navigator::new(self.root_node.clone()),
        };

        analyzer.analyze(ctx).await
    }

    pub async fn diagnostics(self: &Arc<Self>) -> Diagnostics {
        let d = self.run_analyzer(&self.collect_diagnostics, ()).await;

        let mut diagnostics = self.diagnostics.lock().await;

        if !d.is_empty() {
            diagnostics.push_all(d);
        }

        diagnostics.clone()
    }

    pub async fn exported_declarations(self: &Arc<Self>) -> Vec<(String, Arc<Declaration>)> {
        self.run_analyzer(&self.exported_declarations, ()).await
    }

    pub async fn declaration_referenced_by(
        self: &Arc<Self>,
        reference: Arc<ReferenceExpression>,
    ) -> Option<Arc<Declaration>> {
        self.run_analyzer(&self.find_declaration, reference)
            .await
            .ok()
    }

    pub async fn declaration_referenced_by_type(
        self: &Arc<Self>,
        reference: Arc<ReferenceTypeExpression>,
    ) -> Option<Arc<Declaration>> {
        self.run_analyzer(&self.find_type_declaration, reference)
            .await
            .ok()
    }

    pub async fn get_type_of(self: &Arc<Self>, expression: Arc<Expression>) -> Type {
        self.run_analyzer(&self.get_type_of_expression, expression)
            .await
    }

    pub async fn resolve_type(self: &Arc<Self>, expression: Arc<TypeExpression>) -> Type {
        self.run_analyzer(&self.get_type_of_type_expression, expression)
            .await
    }

    pub async fn get_behaviours_of_object(
        self: &Arc<Self>,
        object: Arc<ObjectDeclaration>,
    ) -> Vec<Behaviour> {
        self.run_analyzer(&self.get_behaviours_of_object, object)
            .await
    }

    pub async fn get_behaviours_of_type(self: &Arc<Self>, type_: Type) -> Vec<Behaviour> {
        match type_ {
            Type::Failed { .. } => vec![],
            Type::Integer(Some(i)) => vec![
                Behaviour {
                    selector: Type::Atom(Some("increment!".into())),
                    reply: Type::Integer(Some(i + 1)),
                },
                Behaviour {
                    selector: Type::Integer(None),
                    reply: Type::Integer(None),
                },
            ],
            Type::Integer(None) => vec![
                Behaviour {
                    selector: Type::Atom(Some("increment!".into())),
                    reply: Type::Integer(None),
                },
                Behaviour {
                    selector: Type::Integer(None),
                    reply: Type::Integer(None),
                },
            ],
            Type::Float(_) => vec![],
            Type::Atom(_) => vec![],
            Type::Unbounded(_, _) => vec![],
            Type::Object(o) => self.get_behaviours_of_object(o).await,
        }
    }

    pub async fn get_type_of_pattern(self: &Arc<Self>, pattern: Arc<Pattern>) -> Type {
        match pattern.as_ref() {
            Pattern::Integer(i) => match &i.literal.kind {
                TokenKind::IntegerLiteral(i, _) => Type::Integer(Some(*i)),
                _ => Type::Integer(None),
            },
            Pattern::Nullary(a) => match &a.atom.kind {
                TokenKind::NullaryAtom => Type::Atom(Some(a.atom.lexeme().into())),
                _ => Type::Integer(None),
            },
        }
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?} {:?}", self.source.uri(), self.root_node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;
    use std::collections::HashMap;

    #[tokio::test]
    async fn single_declaration() {
        let host = Host::new(Arc::new(Context::test()));
        host.set(Source::new("test:x", "object X.")).await;
        let module = host.get(&"test:x".into()).await.unwrap();

        let exported: HashMap<_, _> = module.exported_declarations().await.into_iter().collect();
        assert_eq!(exported.len(), 1);
        assert!(exported.get("X").is_some());
    }

    #[tokio::test]
    async fn duplicated_export() {
        let host = Host::new(Arc::new(Context::test()));
        host.set(Source::new("test:x", "object X. object X.")).await;
        let module = host.get(&"test:x".into()).await.unwrap();

        let diagnostics = module.diagnostics().await;
        assert_eq!(diagnostics.len(), 1);
    }
}
