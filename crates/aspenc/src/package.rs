//! Filesystem packages, module namespaces, and whole-package static globals.
use crate::{
    Expr, Loc, Pattern, Selector, Span, Stmt,
    types::{self, Environment, TypedExpression, TypedStatement},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
};

/// Package spans use disjoint virtual line ranges, resolved here for display.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: Vec<(PathBuf, u16, u16)>,
}
impl SourceMap {
    pub fn resolve(&self, span: Span) -> Option<(&Path, Span)> {
        let (path, start, _) = self
            .files
            .iter()
            .find(|(_, start, end)| span.start.line >= *start && span.end.line <= *end)?;
        let mut local = span;
        local.start.line -= start - 1;
        local.end.line -= start - 1;
        Some((path, local))
    }

    fn lexer<'a>(
        &mut self,
        path: &Path,
        source: &'a str,
    ) -> Result<crate::Lexer<'a>, PackageDiagnostic> {
        // Starting the lexer in the file's range relocates every AST location,
        // including nested type annotations and receiver provenance.
        let mut counter = crate::Lexer::new(source);
        while counter.next().is_some() {}
        let lines = counter.position().line;
        let start = self
            .files
            .last()
            .map_or(Some(1), |(_, _, end)| end.checked_add(1));
        let (start, end) = start
            .and_then(|start| start.checked_add(lines - 1).map(|end| (start, end)))
            .ok_or_else(|| {
                error(
                    path,
                    None,
                    "package source positions exceed u16 line capacity",
                )
            })?;
        self.files.push((path.to_owned(), start, end));
        let mut lexer = crate::Lexer::new(source);
        lexer.pos.line = start;
        Ok(lexer)
    }
}

#[derive(Debug)]
pub struct PackageDiagnostic {
    pub path: PathBuf,
    pub span: Option<Span>,
    pub message: String,
    pub type_error: Option<types::TypeError>,
    /// Resolves virtual spans in `type_error`; `path` and `span` are already local.
    pub sources: SourceMap,
}
impl fmt::Display for PackageDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())?;
        if let Some(span) = self.span {
            write!(f, ":{}:{}", span.start.line, span.start.col)?;
        }
        write!(f, ": {}", self.message)
    }
}
impl std::error::Error for PackageDiagnostic {}
#[derive(Debug)]
pub struct CheckedPackage {
    pub globals: Vec<(String, TypedExpression)>,
    pub entry: TypedStatement,
    /// Resolve locations in the typed AST before displaying them to users.
    pub sources: SourceMap,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    name: String,
    #[serde(alias = "source_root", alias = "source-root")]
    source: PathBuf,
    #[serde(default)]
    dependencies: BTreeMap<String, PathBuf>,
    entry: Option<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    actor: String,
    message: String,
}
fn error(path: &Path, span: Option<Span>, message: impl ToString) -> PackageDiagnostic {
    PackageDiagnostic {
        path: path.to_owned(),
        span,
        message: message.to_string(),
        type_error: None,
        sources: SourceMap::default(),
    }
}
fn type_error(path: &Path, span: Option<Span>, cause: types::TypeError) -> PackageDiagnostic {
    let mut diagnostic = error(path, span, &cause);
    diagnostic.type_error = Some(cause);
    diagnostic
}
struct Unit {
    manifest: Manifest,
    path: PathBuf,
}
struct Module {
    path: PathBuf,
    package: String,
    syntax: crate::ModuleSyntax,
}
struct Global {
    path: PathBuf,
    pattern: Loc<Pattern>,
    value: Loc<Expr>,
}
fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}
fn manifest_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join("aspen.yaml")
    } else {
        path.to_owned()
    }
}
fn load_units(
    path: &Path,
    units: &mut BTreeMap<String, Unit>,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<String, PackageDiagnostic> {
    let path = fs::canonicalize(manifest_path(path)).map_err(|e| error(path, None, e))?;
    if seen.contains(&path) {
        return Ok(units
            .values()
            .find(|u| u.path == path)
            .unwrap()
            .manifest
            .name
            .clone());
    }
    let text = fs::read_to_string(&path).map_err(|e| error(&path, None, e))?;
    let manifest: Manifest = serde_yaml::from_str(&text).map_err(|e| error(&path, None, e))?;
    if !identifier(&manifest.name) {
        return Err(error(&path, None, "package name must be an identifier"));
    }
    if units.contains_key(&manifest.name) {
        return Err(error(
            &path,
            None,
            format!("duplicate package name '{}'", manifest.name),
        ));
    }
    let name = manifest.name.clone();
    let dependencies = manifest.dependencies.clone();
    seen.insert(path.clone());
    units.insert(
        name.clone(),
        Unit {
            manifest,
            path: path.clone(),
        },
    );
    for (alias, relative) in dependencies {
        let actual = load_units(&path.parent().unwrap().join(relative), units, seen)?;
        if alias != actual {
            return Err(error(
                &path,
                None,
                format!("dependency '{alias}' declares package name '{actual}'"),
            ));
        }
    }
    Ok(name)
}
fn source_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), PackageDiagnostic> {
    let entries = fs::read_dir(root).map_err(|e| error(root, None, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| error(root, None, e))?;
        let ty = entry
            .file_type()
            .map_err(|e| error(&entry.path(), None, e))?;
        if ty.is_dir() {
            source_files(&entry.path(), files)?;
        } else if ty.is_file() && entry.path().extension().is_some_and(|s| s == "aspen") {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(())
}
fn global_name(pattern: &Loc<Pattern>) -> &str {
    match &pattern.value {
        Pattern::Variable(name) => name,
        Pattern::Annotated { pattern, .. } => global_name(pattern),
        _ => unreachable!("parser validates globals"),
    }
}
fn rename_pattern(pattern: &mut Loc<Pattern>, name: &str) {
    match &mut pattern.value {
        Pattern::Variable(current) => *current = name.into(),
        Pattern::Annotated { pattern, .. } => rename_pattern(pattern, name),
        _ => unreachable!(),
    }
}
fn pattern_names(pattern: &Loc<Pattern>, names: &mut BTreeSet<String>) {
    match &pattern.value {
        Pattern::Variable(name) => {
            names.insert(name.clone());
        }
        Pattern::Annotated { pattern, .. } => pattern_names(pattern, names),
        Pattern::Selector(selector) => {
            for value in selector.values() {
                pattern_names(value, names);
            }
        }
        Pattern::Discard => {}
    }
}
fn resolve_expr(
    expr: &mut Loc<Expr>,
    names: &BTreeMap<String, String>,
    locals: &BTreeSet<String>,
    path: &Path,
) -> Result<(), PackageDiagnostic> {
    match &mut expr.value {
        Expr::Variable(name) if !locals.contains(name) => {
            if let Some(target) = names.get(name) {
                if target.starts_with("@module:") {
                    return Err(error(
                        path,
                        Some(expr.span),
                        "module namespaces are not values",
                    ));
                }
                *name = target.clone();
            } else if name != "syscall" {
                return Err(error(
                    path,
                    Some(expr.span),
                    format!("unbound or private global '{name}'"),
                ));
            }
        }
        Expr::Selector(selector) => match selector {
            Selector::Atomic(_) => {}
            Selector::Operator { value, .. } => resolve_expr(value, names, locals, path)?,
            Selector::Keyword(parts) => {
                for (_, value) in parts {
                    resolve_expr(value, names, locals, path)?;
                }
            }
        },
        Expr::Send { callee, message } => {
            resolve_expr(callee, names, locals, path)?;
            resolve_expr(message, names, locals, path)?;
        }
        Expr::Actor(actor) => {
            for method in &mut actor.methods {
                let mut scope = locals.clone();
                pattern_names(&method.pattern, &mut scope);
                for stmt in &mut method.body {
                    match &mut stmt.value {
                        Stmt::Expr(expr) => resolve_expr(expr, names, &scope, path)?,
                        Stmt::Let(binding) => {
                            resolve_expr(&mut binding.value, names, &scope, path)?;
                            pattern_names(&binding.pattern, &mut scope);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}
// Type names are resolved independently from runtime bindings. Parameter scopes
// include the whole list so forward and guarded recursive bounds retain meaning.
fn resolve_parameters(
    parameters: &mut [Loc<crate::TypeParameterExpr>],
    names: &BTreeMap<String, String>,
    locals: &BTreeSet<String>,
    path: &Path,
) -> Result<BTreeSet<String>, PackageDiagnostic> {
    let mut scope = locals.clone();
    scope.extend(
        parameters
            .iter()
            .map(|parameter| parameter.name.value.clone()),
    );
    for parameter in parameters {
        if let Some(bound) = &mut parameter.upper_bound {
            resolve_type(bound, names, &scope, path)?;
        }
    }
    Ok(scope)
}
fn resolve_type(
    ty: &mut Loc<crate::TypeExpr>,
    names: &BTreeMap<String, String>,
    locals: &BTreeSet<String>,
    path: &Path,
) -> Result<(), PackageDiagnostic> {
    use crate::TypeExpr;
    match &mut ty.value {
        TypeExpr::Variable(name) | TypeExpr::Apply { name, .. } => {
            if !locals.contains(name) {
                *name = names
                    .get(name)
                    .filter(|name| !name.starts_with("@module:"))
                    .cloned()
                    .ok_or_else(|| {
                        type_error(
                            path,
                            Some(ty.span),
                            types::TypeError::UnknownType {
                                name: name.clone(),
                                span: ty.span,
                            },
                        )
                    })?;
            }
            if let TypeExpr::Apply { arguments, .. } = &mut ty.value {
                for argument in arguments {
                    resolve_type(argument, names, locals, path)?;
                }
            }
        }
        TypeExpr::Actor(methods) => {
            for method in methods {
                let scope = resolve_parameters(&mut method.parameters, names, locals, path)?;
                resolve_type(&mut method.input, names, &scope, path)?;
                if let Some(reply) = &mut method.reply {
                    resolve_type(reply, names, &scope, path)?;
                }
            }
        }
        TypeExpr::Selector(selector) => match selector {
            Selector::Atomic(_) => {}
            Selector::Operator { value, .. } => resolve_type(value, names, locals, path)?,
            Selector::Keyword(parts) => {
                for (_, value) in parts {
                    resolve_type(value, names, locals, path)?;
                }
            }
        },
        _ => {}
    }
    Ok(())
}
fn resolve_pattern_types(
    pattern: &mut Loc<Pattern>,
    names: &BTreeMap<String, String>,
    locals: &BTreeSet<String>,
    path: &Path,
) -> Result<(), PackageDiagnostic> {
    match &mut pattern.value {
        Pattern::Annotated { ty, pattern } => {
            resolve_type(ty, names, locals, path)?;
            resolve_pattern_types(pattern, names, locals, path)?;
        }
        Pattern::Selector(selector) => match selector {
            Selector::Atomic(_) => {}
            Selector::Operator { value, .. } => resolve_pattern_types(value, names, locals, path)?,
            Selector::Keyword(parts) => {
                for (_, value) in parts {
                    resolve_pattern_types(value, names, locals, path)?;
                }
            }
        },
        _ => {}
    }
    Ok(())
}
fn resolve_expr_types(
    expr: &mut Loc<Expr>,
    names: &BTreeMap<String, String>,
    locals: &BTreeSet<String>,
    path: &Path,
) -> Result<(), PackageDiagnostic> {
    match &mut expr.value {
        Expr::Actor(actor) => {
            for method in &mut actor.methods {
                let scope = resolve_parameters(&mut method.parameters, names, locals, path)?;
                resolve_pattern_types(&mut method.pattern, names, &scope, path)?;
                if let Some(reply) = &mut method.reply {
                    resolve_type(reply, names, &scope, path)?;
                }
                for stmt in &mut method.body {
                    match &mut stmt.value {
                        Stmt::Expr(expr) => resolve_expr_types(expr, names, &scope, path)?,
                        Stmt::Let(binding) => {
                            resolve_pattern_types(&mut binding.pattern, names, &scope, path)?;
                            resolve_expr_types(&mut binding.value, names, &scope, path)?;
                        }
                    }
                }
            }
        }
        Expr::Send { callee, message } => {
            resolve_expr_types(callee, names, locals, path)?;
            resolve_expr_types(message, names, locals, path)?;
        }
        Expr::Selector(selector) => match selector {
            Selector::Atomic(_) => {}
            Selector::Operator { value, .. } => resolve_expr_types(value, names, locals, path)?,
            Selector::Keyword(parts) => {
                for (_, value) in parts {
                    resolve_expr_types(value, names, locals, path)?;
                }
            }
        },
        _ => {}
    }
    Ok(())
}
fn static_refs(
    expr: &Loc<Expr>,
    refs: &mut BTreeSet<String>,
    path: &Path,
) -> Result<(), PackageDiagnostic> {
    match &expr.value {
        Expr::Variable(name) => {
            if name != "syscall" {
                refs.insert(name.clone());
            }
        }
        Expr::Selector(selector) => {
            for value in selector.values() {
                static_refs(value, refs, path)?;
            }
        }
        Expr::Send { .. } | Expr::ReplyTo => {
            return Err(error(
                path,
                Some(expr.span),
                "global initializers must be static values; sends are not allowed",
            ));
        }
        _ => {}
    }
    Ok(())
}
// Receiver signatures do not depend on method bodies. Temporarily detach bodies
// to infer all global interfaces before checking any mutually recursive actors.
fn detach_bodies(expr: &mut Loc<Expr>, bodies: &mut Vec<Vec<Loc<Stmt>>>) {
    match &mut expr.value {
        Expr::Actor(actor) => {
            for method in &mut actor.methods {
                bodies.push(std::mem::take(&mut method.body));
            }
        }
        Expr::Selector(selector) => match selector {
            Selector::Atomic(_) => {}
            Selector::Operator { value, .. } => detach_bodies(value, bodies),
            Selector::Keyword(parts) => {
                for (_, value) in parts {
                    detach_bodies(value, bodies);
                }
            }
        },
        _ => {}
    }
}
fn restore_bodies(expr: &mut Loc<Expr>, bodies: &mut impl Iterator<Item = Vec<Loc<Stmt>>>) {
    match &mut expr.value {
        Expr::Actor(actor) => {
            for method in &mut actor.methods {
                method.body = bodies.next().unwrap();
            }
        }
        Expr::Selector(selector) => match selector {
            Selector::Atomic(_) => {}
            Selector::Operator { value, .. } => restore_bodies(value, bodies),
            Selector::Keyword(parts) => {
                for (_, value) in parts {
                    restore_bodies(value, bodies);
                }
            }
        },
        _ => {}
    }
}
/// Strongly connected components in dependency-first order. Cycles in module
/// imports are legal; only cycles in static value construction are rejected.
fn components(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        seen: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if !seen.insert(node.into()) {
            return;
        }
        if let Some(edges) = graph.get(node) {
            for next in edges {
                visit(next, graph, seen, order);
            }
        }
        order.push(node.into());
    }
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for node in graph.keys() {
        visit(node, graph, &mut seen, &mut order);
    }
    let mut reverse: BTreeMap<String, BTreeSet<String>> =
        graph.keys().map(|k| (k.clone(), BTreeSet::new())).collect();
    for (node, edges) in graph {
        for edge in edges {
            reverse
                .entry(edge.clone())
                .or_default()
                .insert(node.clone());
        }
    }
    seen.clear();
    let mut result = Vec::new();
    for node in order.into_iter().rev() {
        if seen.contains(&node) {
            continue;
        }
        let mut component = Vec::new();
        visit(&node, &reverse, &mut seen, &mut component);
        component.sort();
        result.push(component);
    }
    result.reverse();
    result
}
fn add_name(
    names: &mut BTreeMap<String, String>,
    alias: String,
    target: String,
    path: &Path,
    span: Span,
) -> Result<(), PackageDiagnostic> {
    if names.insert(alias.clone(), target).is_some() {
        return Err(error(
            path,
            Some(span),
            format!("duplicate binding '{alias}'"),
        ));
    }
    Ok(())
}
pub fn load_and_check(path: impl AsRef<Path>) -> Result<CheckedPackage, Vec<PackageDiagnostic>> {
    let mut sources = SourceMap::default();
    check_package(path.as_ref(), &mut sources).map_err(|mut e| {
        if let Some((path, span)) = e.span.and_then(|span| sources.resolve(span)) {
            e.path = path.to_owned();
            e.span = Some(span);
        }
        e.sources = sources;
        vec![e]
    })
}
fn check_package(
    path: &Path,
    sources: &mut SourceMap,
) -> Result<CheckedPackage, PackageDiagnostic> {
    let mut units = BTreeMap::new();
    let root = load_units(path, &mut units, &mut BTreeSet::new())?;
    let mut modules = BTreeMap::new();
    for (package, unit) in &units {
        let source_root = unit.path.parent().unwrap().join(&unit.manifest.source);
        let mut files = Vec::new();
        source_files(&source_root, &mut files)?;
        for file in files {
            let relative = file.strip_prefix(&source_root).unwrap().with_extension("");
            let mut parts: Vec<_> = relative
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            if parts.last().is_some_and(|s| s == "index") {
                parts.pop();
            }
            if parts.iter().any(|s| !identifier(s)) {
                return Err(error(
                    &file,
                    None,
                    "module path components must be identifiers",
                ));
            }
            let name = std::iter::once(package.clone())
                .chain(parts)
                .collect::<Vec<_>>()
                .join("/");
            if modules.contains_key(&name) {
                return Err(error(
                    &file,
                    None,
                    format!("module path collision for '{name}'"),
                ));
            }
            let source = fs::read_to_string(&file).map_err(|e| error(&file, None, e))?;
            let mut diagnostics = Vec::new();
            let syntax = crate::parse_module(sources.lexer(&file, &source)?, &mut diagnostics);
            if let Some(diagnostic) = diagnostics.into_iter().next() {
                return Err(error(&file, Some(diagnostic.span), diagnostic.message));
            }
            modules.insert(
                name,
                Module {
                    path: file,
                    package: package.clone(),
                    syntax,
                },
            );
        }
    }
    let mut exports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut type_exports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, module) in &modules {
        type_exports.insert(
            name.clone(),
            module
                .syntax
                .aliases
                .iter()
                .filter(|alias| alias.exported)
                .map(|alias| alias.name.value.clone())
                .collect(),
        );
        let mut public = BTreeSet::new();
        for global in &module.syntax.globals {
            if global.exported {
                public.insert(global_name(&global.binding.pattern).to_owned());
            }
        }
        exports.insert(name.clone(), public);
    }
    let mut graph = BTreeMap::new();
    let mut globals = BTreeMap::new();
    let mut aliases = Vec::new();
    for (module_name, module) in modules {
        let mut type_names = BTreeMap::new();
        for alias in &module.syntax.aliases {
            if matches!(
                alias.name.value.as_str(),
                "never"
                    | "bytes"
                    | "string"
                    | "int"
                    | "float"
                    | "selector"
                    | "atom"
                    | "optagged"
                    | "keywordtagged"
            ) {
                return Err(error(
                    &module.path,
                    Some(alias.name.span),
                    "type alias cannot redefine a primitive type",
                ));
            }
            add_name(
                &mut type_names,
                alias.name.value.clone(),
                format!("{module_name}/{}", alias.name.value),
                &module.path,
                alias.span,
            )?;
        }
        let mut names = BTreeMap::new();
        for global in &module.syntax.globals {
            add_name(
                &mut names,
                global_name(&global.binding.pattern).to_owned(),
                format!("{module_name}/{}", global_name(&global.binding.pattern)),
                &module.path,
                global.span,
            )?;
        }
        let mut edges = BTreeSet::new();
        for import in &module.syntax.imports {
            let target = &import.path.value;
            let dependency = target.split('/').next().unwrap_or("");
            if dependency != module.package
                && !units[&module.package]
                    .manifest
                    .dependencies
                    .contains_key(dependency)
            {
                return Err(error(
                    &module.path,
                    Some(import.span),
                    format!("undeclared package dependency '{dependency}'"),
                ));
            }
            let public = exports.get(target).ok_or_else(|| {
                error(
                    &module.path,
                    Some(import.span),
                    format!("unknown module '{target}'"),
                )
            })?;
            edges.insert(target.clone());
            match &import.binding {
                crate::ImportBinding::Module(alias) => {
                    let alias = &alias.value;
                    add_name(
                        &mut names,
                        alias.clone(),
                        format!("@module:{target}"),
                        &module.path,
                        import.span,
                    )?;
                    add_name(
                        &mut type_names,
                        alias.clone(),
                        format!("@module:{target}"),
                        &module.path,
                        import.span,
                    )?;
                    for name in &type_exports[target] {
                        add_name(
                            &mut type_names,
                            format!("{alias}/{name}"),
                            format!("{target}/{name}"),
                            &module.path,
                            import.span,
                        )?;
                    }
                    for name in public {
                        add_name(
                            &mut names,
                            format!("{alias}/{name}"),
                            format!("{target}/{name}"),
                            &module.path,
                            import.span,
                        )?;
                    }
                }
                crate::ImportBinding::Names(selected) => {
                    for selected in selected {
                        let public = if selected.is_type {
                            &type_exports[target]
                        } else {
                            public
                        };
                        if !public.contains(&selected.name.value) {
                            return Err(error(
                                &module.path,
                                Some(import.span),
                                format!("'{}' is not exported by '{target}'", selected.name.value),
                            ));
                        }
                        add_name(
                            if selected.is_type {
                                &mut type_names
                            } else {
                                &mut names
                            },
                            selected.alias.value.clone(),
                            format!("{target}/{}", selected.name.value),
                            &module.path,
                            import.span,
                        )?;
                    }
                }
            }
        }
        graph.insert(module_name.clone(), edges);
        for mut alias in module.syntax.aliases {
            let scope = resolve_parameters(
                &mut alias.parameters,
                &type_names,
                &BTreeSet::new(),
                &module.path,
            )?;
            resolve_type(&mut alias.body, &type_names, &scope, &module.path)?;
            alias.name.value = format!("{module_name}/{}", alias.name.value);
            aliases.push(alias);
        }
        for global in module.syntax.globals {
            let global = global.value;
            let name = global_name(&global.binding.pattern).to_owned();
            let mut value = *global.binding.value;
            resolve_expr(&mut value, &names, &BTreeSet::new(), &module.path)?;
            resolve_expr_types(&mut value, &type_names, &BTreeSet::new(), &module.path)?;
            let mut pattern = global.binding.pattern;
            resolve_pattern_types(&mut pattern, &type_names, &BTreeSet::new(), &module.path)?;
            rename_pattern(&mut pattern, &format!("{module_name}/{name}"));
            globals.insert(
                format!("{module_name}/{name}"),
                Global {
                    path: module.path.clone(),
                    pattern,
                    value,
                },
            );
        }
    }
    let module_order = components(&graph);
    let mut pending: Vec<_> = module_order
        .into_iter()
        .flatten()
        .flat_map(|module| {
            globals
                .keys()
                .filter(move |name| {
                    name.rsplit_once('/')
                        .is_some_and(|(owner, _)| owner == module)
                })
                .cloned()
        })
        .collect();
    let checking_order = pending.clone();
    let mut references = BTreeMap::new();
    for (name, global) in &globals {
        let mut refs = BTreeSet::new();
        static_refs(&global.value, &mut refs, &global.path)?;
        references.insert(name.clone(), refs);
    }
    let mut environment = Environment::default();
    environment.types = environment.types.with_aliases(&aliases).map_err(|cause| {
        let span = match &cause {
            types::TypeError::UnknownType { span, .. }
            | types::TypeError::InvalidAnnotation { span, .. }
            | types::TypeError::InvalidActorType { span } => Some(*span),
            _ => aliases.first().map(|alias| alias.span),
        };
        type_error(path, span, cause)
    })?;
    let mut order = Vec::new();
    while !pending.is_empty() {
        let Some(index) = pending.iter().position(|name| {
            references[name]
                .iter()
                .all(|r| environment.lookup(r).is_some())
        }) else {
            let name = &pending[0];
            let global = &globals[name];
            return Err(error(
                &global.path,
                Some(global.value.span),
                format!("cyclic global initializer involving '{name}'"),
            ));
        };
        let name = pending.remove(index);
        let global = globals.get_mut(&name).unwrap();
        let mut bodies = Vec::new();
        detach_bodies(&mut global.value, &mut bodies);
        let result = types::check_binding(&environment, &global.pattern, &global.value);
        restore_bodies(&mut global.value, &mut bodies.into_iter());
        let checked = result.map_err(|e| type_error(&global.path, Some(global.value.span), e))?;
        environment = environment.extended(checked.bindings);
        order.push(name);
    }
    let mut typed = BTreeMap::new();
    for name in checking_order {
        let global = &globals[&name];
        let checked = types::check_binding(&environment, &global.pattern, &global.value)
            .map_err(|e| type_error(&global.path, Some(global.value.span), e))?;
        environment = environment.extended(checked.bindings);
        typed.insert(name, checked.value);
    }
    let unit = &units[&root];
    let entry = unit.manifest.entry.as_ref().ok_or_else(|| {
        error(
            &unit.path,
            None,
            "root package requires entry: {actor: package/module/name, message: start}",
        )
    })?;

    let Some((owner, name)) = entry.actor.rsplit_once('/') else {
        return Err(error(
            &unit.path,
            None,
            "entry actor must be a qualified exported global",
        ));
    };
    if !exports.get(owner).is_some_and(|names| names.contains(name)) {
        return Err(error(&unit.path, None, "entry actor must be exported"));
    }
    if environment.lookup(&entry.actor).is_none() {
        return Err(error(
            &unit.path,
            None,
            format!("unknown entry actor '{}'", entry.actor),
        ));
    }
    let message_source = format!(
        "{}.",
        if entry.message.starts_with('#') {
            entry.message.clone()
        } else {
            format!("#{}", entry.message)
        }
    );
    let mut diagnostics = Vec::new();
    let lexer = sources.lexer(&unit.path, &message_source)?;
    let span = Span {
        start: lexer.position(),
        end: lexer.position(),
    };
    let mut program = crate::parse(lexer, &mut diagnostics);
    if !diagnostics.is_empty() || program.statements.len() != 1 {
        return Err(error(
            &unit.path,
            None,
            "entry message must be a static selector",
        ));
    }
    let Stmt::Expr(message) = program.statements.remove(0).value else {
        return Err(error(&unit.path, None, "invalid entry message"));
    };
    if !matches!(message.value, Expr::Selector(Selector::Atomic(_))) {
        return Err(error(&unit.path, None, "entry message must be a selector"));
    }
    let mut refs = BTreeSet::new();
    static_refs(&message, &mut refs, &unit.path)?;
    if !refs.is_empty() {
        return Err(error(
            &unit.path,
            None,
            "entry message cannot reference globals",
        ));
    }
    let expression = Loc {
        span,
        value: Expr::Send {
            callee: Box::new(Loc {
                span,
                value: Expr::Variable(entry.actor.clone()),
            }),
            message: Box::new(message),
        },
    };
    let mut statements = types::check_statements_in(
        &environment,
        &[Loc {
            span,
            value: Stmt::Expr(expression),
        }],
    )
    .map_err(|e| type_error(&unit.path, None, e))?;
    if !matches!(statements[0], TypedStatement::NoReplySend { .. }) {
        return Err(error(
            &unit.path,
            None,
            "entry actor must accept the message without a reply",
        ));
    }
    Ok(CheckedPackage {
        globals: order
            .into_iter()
            .map(|name| {
                let value = typed.remove(&name).unwrap();
                (name, value)
            })
            .collect(),
        entry: statements.remove(0),
        sources: sources.clone(),
    })
}
