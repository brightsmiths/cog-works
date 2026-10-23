//! Handlebars-based template engine implementation.
//!
//! Provides [`HandlebarsTemplateEngine`], which implements
//! [`pipeline::TemplateEngine`] using the `handlebars` crate.
//!
//! ## Template Location
//!
//! Templates are loaded from `.cogworks/templates/` in the repository
//! working directory. Each template is a Handlebars file with the `.hbs`
//! extension. A manifest file at `.cogworks/templates/manifest.toml` declares
//! the required variable names for each template:
//!
//! ```toml
//! [templates.pr-body]
//! required_vars = ["issue_number", "branch_name", "summary"]
//!
//! [templates.node-comment]
//! required_vars = ["node_id", "status", "timestamp"]
//! ```
//!
//! ## Lifecycle
//!
//! The `HandlebarsTemplateEngine` is owned by the `nodes` crate and created
//! once in the `cli` composition root. All templates are pre-loaded at startup
//! via [`HandlebarsTemplateEngine::register_template`]; rendering is
//! synchronous in-memory work with no I/O.
//!
//! ## Architectural Note
//!
//! The `HandlebarsTemplateEngine` is an internal implementation detail of the
//! `nodes` crate. It is injected into nodes that require it as
//! `Arc<dyn pipeline::TemplateEngine>`. No other crate depends on the
//! `handlebars` crate directly.
//!
//! ## Specification
//!
//! See `docs/spec/interfaces/github-traits.md` §`TemplateEngine` and
//! `docs/spec/interfaces/infrastructure.md` §`HandlebarsTemplateEngine`.

use std::collections::HashMap;

use handlebars::Handlebars;
use pipeline::{TemplateEngine, TemplateError};

// ─── HandlebarsTemplateEngine ────────────────────────────────────────────────

/// Handlebars-based implementation of [`pipeline::TemplateEngine`].
///
/// Templates are registered via [`register_template`](Self::register_template)
/// at startup. Rendering and variable listing are synchronous and allocation-
/// bounded.
///
/// All variable values are `String` — the template engine performs no type
/// coercion.
///
/// ## Thread Safety
///
/// `HandlebarsTemplateEngine` implements `Send + Sync` (inherited from the
/// `handlebars::Handlebars` registry which is thread-safe after all templates
/// are registered). The `required_vars` map is populated at construction time
/// only; no mutation occurs during rendering.
///
/// See `docs/spec/interfaces/infrastructure.md` §`HandlebarsTemplateEngine`.
pub struct HandlebarsTemplateEngine {
    /// The Handlebars template registry, pre-loaded at startup.
    registry: Handlebars<'static>,
    /// Declared required variable names per template name.
    ///
    /// Populated from the template manifest at construction time; never mutated
    /// during rendering.
    required_vars: HashMap<String, Vec<String>>,
}

impl HandlebarsTemplateEngine {
    /// Creates a new `HandlebarsTemplateEngine` with an empty registry.
    ///
    /// Templates are registered individually via
    /// [`register_template`](Self::register_template). The engine is ready
    /// for rendering after all templates are registered.
    ///
    /// HTML escaping is disabled on the underlying registry: by default
    /// `handlebars::Handlebars` rewrites `&`, `<`, `>`, and `"` in
    /// substituted values into HTML entities, which contradicts this
    /// engine's "literal substitution, no coercion" contract and would
    /// corrupt the GitHub Markdown PR bodies and issue comments it renders.
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Handlebars::new();
        registry.register_escape_fn(handlebars::no_escape);
        Self {
            registry,
            required_vars: HashMap::new(),
        }
    }

    /// Registers a named template and its required variable names.
    ///
    /// # Arguments
    ///
    /// * `name` — Unique identifier for the template (e.g. `"pr-body"`).
    /// * `template_source` — Handlebars template source (`.hbs` content).
    /// * `required` — Names of all variables that must be present in the context
    ///   map when [`TemplateEngine::render`] is called.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError::SyntaxError`] if the Handlebars source contains
    /// a syntax error.
    pub fn register_template(
        &mut self,
        name: impl Into<String>,
        template_source: &str,
        required: Vec<String>,
    ) -> Result<(), TemplateError> {
        let name = name.into();

        // `register_template_string` compiles the source before inserting it
        // into the registry's internal map; on a parse failure nothing is
        // registered, so there is no risk of leaving a partially-registered
        // template behind.
        self.registry
            .register_template_string(&name, template_source)
            .map_err(|error| TemplateError::SyntaxError {
                name: name.clone(),
                message: error.to_string(),
            })?;

        self.required_vars.insert(name, required);
        Ok(())
    }

    /// Looks up the declared required-variable list for `name`.
    ///
    /// Shared by [`TemplateEngine::render`] and
    /// [`TemplateEngine::list_required_variables`] so both report an
    /// identical [`TemplateError::NotFound`] for an unregistered template
    /// name instead of duplicating the lookup-or-error logic.
    fn required_vars_for(&self, name: &str) -> Result<&Vec<String>, TemplateError> {
        self.required_vars
            .get(name)
            .ok_or_else(|| TemplateError::NotFound {
                name: name.to_string(),
            })
    }

    /// Builds a [`TemplateError::ConstraintViolation`] for `name`.
    ///
    /// Centralizes the error shape used by [`TemplateEngine::render`] for
    /// both the handlebars runtime-error case and the empty-output case, so
    /// the two call sites cannot drift in field construction.
    fn constraint_violation(name: &str, message: impl Into<String>) -> TemplateError {
        TemplateError::ConstraintViolation {
            name: name.to_string(),
            message: message.into(),
        }
    }
}

impl Default for HandlebarsTemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngine for HandlebarsTemplateEngine {
    /// Render a named template with the provided variable context.
    ///
    /// Validates that all required variables declared for `name` are present
    /// in `context` before rendering. Returns
    /// [`TemplateError::MissingVariables`] if any required variable is absent.
    ///
    /// # Errors
    ///
    /// * [`TemplateError::NotFound`] — `name` is not registered.
    /// * [`TemplateError::MissingVariables`] — one or more required variables
    ///   are absent from `context`.
    /// * [`TemplateError::ConstraintViolation`] — rendered output is empty.
    ///
    /// See `docs/spec/interfaces/github-traits.md` §`TemplateEngine`.
    fn render(
        &self,
        name: &str,
        context: HashMap<String, String>,
    ) -> Result<String, TemplateError> {
        let required = self.required_vars_for(name)?;

        let missing: Vec<String> = required
            .iter()
            .filter(|variable| !context.contains_key(*variable))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(TemplateError::MissingVariables { missing });
        }

        // All declared required variables are present and the template body
        // was validated for syntax at registration time, so a render-time
        // handlebars error here indicates an unexpected runtime condition
        // (e.g. a resource limit) rather than a usage error — report it as a
        // constraint violation rather than leaking the raw handlebars error
        // detail.
        let rendered = self
            .registry
            .render(name, &context)
            .map_err(|error| Self::constraint_violation(name, error.to_string()))?;

        if rendered.is_empty() {
            return Err(Self::constraint_violation(name, "rendered output is empty"));
        }

        Ok(rendered)
    }

    /// Returns the list of variable names required by the named template.
    ///
    /// # Errors
    ///
    /// * [`TemplateError::NotFound`] — `name` is not registered.
    ///
    /// See `docs/spec/interfaces/github-traits.md` §`TemplateEngine`.
    fn list_required_variables(&self, name: &str) -> Result<Vec<String>, TemplateError> {
        self.required_vars_for(name).cloned()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Adversarial test suite for [`HandlebarsTemplateEngine`].
///
/// ## Phase: RED
///
/// `register_template`, `render`, and `list_required_variables` are
/// `todo!()` stubs. Every test here is expected to **compile** cleanly but
/// **panic** at runtime until the implementation lands (GREEN phase).
///
/// ## Why these fixtures
///
/// Tests avoid asserting on any detail not pinned by the interface contract
/// (e.g. iteration order of `required_vars` is deliberately checked via
/// sorted comparison, not literal `Vec` equality, since the contract does
/// not promise insertion-order preservation for that accessor). Every test
/// asserts on an *exact* rendered string, error variant, and error payload
/// so that a stub returning a hardcoded constant, an empty `Ok(String::new())`,
/// or a swapped error variant fails immediately.
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pipeline::TemplateError;
    use proptest::prelude::*;

    use super::{HandlebarsTemplateEngine, TemplateEngine};

    // ─── Test helpers ────────────────────────────────────────────────────────

    /// Builds a context map from `(key, value)` pairs — avoids repeating
    /// `HashMap::new()` + `.insert()` boilerplate across every test.
    fn ctx(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Asserts a type is `Send + Sync` at compile time; used by the
    /// thread-safety contract test.
    fn assert_send_sync<T: Send + Sync>() {}

    // ═══════════════════════════════════════════════════════════════════════
    // Tier 1: Specification tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_register_template_valid_syntax_returns_ok() {
        let mut engine = HandlebarsTemplateEngine::new();

        let result = engine.register_template("greet", "Hello {{name}}!", vec!["name".into()]);

        assert!(result.is_ok());
    }

    #[test]
    fn test_render_registered_template_substitutes_variable() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "Hello {{name}}!", vec!["name".into()])
            .expect("registration must succeed for valid syntax");

        let rendered = engine
            .render("greet", ctx(&[("name", "Alice")]))
            .expect("render must succeed when required variables are present");

        assert_eq!(rendered, "Hello Alice!");
    }

    #[test]
    fn test_render_unregistered_name_returns_not_found() {
        let engine = HandlebarsTemplateEngine::new();

        let result = engine.render("does-not-exist", HashMap::new());

        match result {
            Err(TemplateError::NotFound { name }) => assert_eq!(name, "does-not-exist"),
            other => panic!("expected TemplateError::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_register_template_invalid_syntax_returns_syntax_error() {
        let mut engine = HandlebarsTemplateEngine::new();

        // Unclosed `{{#if}}` block — handlebars requires a matching
        // `{{/if}}`; omitting it is an unrecoverable parse error, not a
        // runtime condition.
        let result = engine.register_template("broken", "{{#if cond}}no closing tag", vec![]);

        match result {
            Err(TemplateError::SyntaxError { name, message }) => {
                assert_eq!(name, "broken");
                assert!(
                    !message.is_empty(),
                    "syntax error message must not be empty"
                );
            }
            other => panic!("expected TemplateError::SyntaxError, got {other:?}"),
        }
    }

    #[test]
    fn test_render_missing_required_variable_returns_missing_variables() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "Hello {{name}}!", vec!["name".into()])
            .expect("registration must succeed for valid syntax");

        let result = engine.render("greet", HashMap::new());

        match result {
            Err(TemplateError::MissingVariables { missing }) => {
                assert_eq!(missing, vec!["name".to_string()]);
            }
            other => panic!("expected TemplateError::MissingVariables, got {other:?}"),
        }
    }

    #[test]
    fn test_render_literally_empty_template_returns_constraint_violation() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("blank", "", vec![])
            .expect("registration of an empty-but-syntactically-valid template must succeed");

        let result = engine.render("blank", HashMap::new());

        match result {
            Err(TemplateError::ConstraintViolation { name, message }) => {
                assert_eq!(name, "blank");
                assert!(!message.is_empty());
            }
            other => panic!("expected TemplateError::ConstraintViolation, got {other:?}"),
        }
    }

    #[test]
    fn test_render_conditional_template_producing_empty_output_returns_constraint_violation() {
        let mut engine = HandlebarsTemplateEngine::new();
        // `flag` is deliberately NOT declared as required: the block is
        // skipped when the context omits it, yielding empty output without
        // tripping `MissingVariables`.
        engine
            .register_template("maybe", "{{#if flag}}present{{/if}}", vec![])
            .expect("registration must succeed for valid syntax");

        let result = engine.render("maybe", HashMap::new());

        match result {
            Err(TemplateError::ConstraintViolation { name, .. }) => assert_eq!(name, "maybe"),
            other => panic!("expected TemplateError::ConstraintViolation, got {other:?}"),
        }
    }

    #[test]
    fn test_list_required_variables_known_template_returns_declared_list() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template(
                "pr-body",
                "{{issue_number}} {{branch_name}} {{summary}}",
                vec![
                    "issue_number".into(),
                    "branch_name".into(),
                    "summary".into(),
                ],
            )
            .expect("registration must succeed for valid syntax");

        let mut required = engine
            .list_required_variables("pr-body")
            .expect("template is registered");
        required.sort();

        assert_eq!(required, vec!["branch_name", "issue_number", "summary"]);
    }

    #[test]
    fn test_list_required_variables_unknown_template_returns_not_found() {
        let engine = HandlebarsTemplateEngine::new();

        let result = engine.list_required_variables("never-registered");

        match result {
            Err(TemplateError::NotFound { name }) => assert_eq!(name, "never-registered"),
            other => panic!("expected TemplateError::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_register_template_duplicate_name_overwrites_previous_definition() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "A: {{a}}", vec!["a".into()])
            .expect("first registration must succeed");
        engine
            .register_template("greet", "B: {{b}}", vec!["b".into()])
            .expect("second registration must succeed and replace the first");

        let rendered = engine
            .render("greet", ctx(&[("b", "replaced")]))
            .expect("render must use the second registration's template and required vars");

        assert_eq!(rendered, "B: replaced");
    }

    #[test]
    fn test_list_required_variables_reflects_latest_registration() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "A: {{a}}", vec!["a".into()])
            .expect("first registration must succeed");
        engine
            .register_template("greet", "B: {{b}}", vec!["b".into()])
            .expect("second registration must succeed");

        let required = engine
            .list_required_variables("greet")
            .expect("template is registered");

        assert_eq!(required, vec!["b".to_string()]);
    }

    #[test]
    fn test_default_produces_equivalent_engine_to_new() {
        let mut from_new = HandlebarsTemplateEngine::new();
        let mut from_default = HandlebarsTemplateEngine::default();

        from_new
            .register_template("t", "Hi {{x}}", vec!["x".into()])
            .expect("registration must succeed on an engine built via new()");
        from_default
            .register_template("t", "Hi {{x}}", vec!["x".into()])
            .expect("registration must succeed on an engine built via default()");

        let rendered_new = from_new
            .render("t", ctx(&[("x", "Bob")]))
            .expect("render must succeed");
        let rendered_default = from_default
            .render("t", ctx(&[("x", "Bob")]))
            .expect("render must succeed");

        assert_eq!(rendered_new, rendered_default);
        assert_eq!(rendered_new, "Hi Bob");
    }

    #[test]
    fn test_render_multiple_variables_all_substituted() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template(
                "node-comment",
                "{{node_id}}|{{status}}|{{timestamp}}",
                vec!["node_id".into(), "status".into(), "timestamp".into()],
            )
            .expect("registration must succeed for valid syntax");

        let rendered = engine
            .render(
                "node-comment",
                ctx(&[
                    ("node_id", "n-42"),
                    ("status", "complete"),
                    ("timestamp", "2026-01-01T00:00:00Z"),
                ]),
            )
            .expect("render must succeed when all required variables are present");

        assert_eq!(rendered, "n-42|complete|2026-01-01T00:00:00Z");
    }

    #[test]
    fn test_render_unused_required_variable_still_succeeds() {
        let mut engine = HandlebarsTemplateEngine::new();
        // `unused` is declared required but never referenced in the
        // template body — the required-vars list is a pre-render
        // validation gate, not a template-usage check.
        engine
            .register_template(
                "greet",
                "Hello {{name}}!",
                vec!["name".into(), "unused".into()],
            )
            .expect("registration must succeed for valid syntax");

        let rendered = engine
            .render(
                "greet",
                ctx(&[("name", "Carol"), ("unused", "ignored-value")]),
            )
            .expect("render must succeed once every declared required variable is present");

        assert_eq!(rendered, "Hello Carol!");
    }

    #[test]
    fn test_render_ignores_extra_context_variables() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "Hello {{name}}!", vec!["name".into()])
            .expect("registration must succeed for valid syntax");

        let rendered = engine
            .render(
                "greet",
                ctx(&[
                    ("name", "Dave"),
                    ("extra_unrequired_key", "should-be-ignored"),
                ]),
            )
            .expect("extra context keys beyond the declared required set must not error");

        assert_eq!(rendered, "Hello Dave!");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Tier 2: Adversarial / boundary tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_render_missing_variables_error_lists_exactly_absent_vars() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template(
                "trio",
                "{{a}}{{b}}{{c}}",
                vec!["a".into(), "b".into(), "c".into()],
            )
            .expect("registration must succeed for valid syntax");

        // Only "b" is supplied; "a" and "c" are absent.
        let result = engine.render("trio", ctx(&[("b", "present")]));

        match result {
            Err(TemplateError::MissingVariables { mut missing }) => {
                missing.sort();
                assert_eq!(missing, vec!["a".to_string(), "c".to_string()]);
            }
            other => panic!("expected TemplateError::MissingVariables, got {other:?}"),
        }
    }

    #[test]
    fn test_register_template_empty_required_list_renders_with_empty_context() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("static", "no variables here", vec![])
            .expect("registration must succeed for valid syntax");

        let rendered = engine
            .render("static", HashMap::new())
            .expect("a template with no required variables must render with an empty context");

        assert_eq!(rendered, "no variables here");
    }

    #[test]
    fn test_render_reuses_single_registry_across_multiple_templates() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("first", "First: {{x}}", vec!["x".into()])
            .expect("first registration must succeed");
        engine
            .register_template("second", "Second: {{y}}", vec!["y".into()])
            .expect("second registration must succeed");

        let first = engine
            .render("first", ctx(&[("x", "one")]))
            .expect("rendering 'first' must succeed");
        let second = engine
            .render("second", ctx(&[("y", "two")]))
            .expect("rendering 'second' must succeed from the same engine instance");

        assert_eq!(first, "First: one");
        assert_eq!(second, "Second: two");
    }

    #[test]
    fn test_required_vars_not_mutated_by_repeated_render_calls() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "Hello {{name}}!", vec!["name".into()])
            .expect("registration must succeed for valid syntax");

        // Render several times, including once with a missing variable, then
        // confirm `list_required_variables` still reflects exactly what was
        // declared at registration — `required_vars` must never be mutated
        // by `render`, success or failure.
        let _ = engine.render("greet", ctx(&[("name", "Frank")]));
        let _ = engine.render("greet", HashMap::new());
        let _ = engine.render("greet", ctx(&[("name", "Grace")]));

        let required = engine
            .list_required_variables("greet")
            .expect("template is registered");

        assert_eq!(required, vec!["name".to_string()]);
    }

    #[test]
    fn test_render_same_template_repeatedly_without_reregistration_is_consistent() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "Hello {{name}}!", vec!["name".into()])
            .expect("registration must succeed for valid syntax");

        let first_call = engine
            .render("greet", ctx(&[("name", "Eve")]))
            .expect("first render must succeed");
        let second_call = engine
            .render("greet", ctx(&[("name", "Eve")]))
            .expect("second render without re-registration must succeed identically");

        assert_eq!(first_call, second_call);
        assert_eq!(first_call, "Hello Eve!");
    }

    #[test]
    fn test_render_not_found_error_name_matches_requested_name_for_distinct_inputs() {
        let engine = HandlebarsTemplateEngine::new();

        let first = engine.render("alpha-missing", HashMap::new());
        let second = engine.render("beta-missing", HashMap::new());

        match (first, second) {
            (
                Err(TemplateError::NotFound { name: first_name }),
                Err(TemplateError::NotFound { name: second_name }),
            ) => {
                assert_eq!(first_name, "alpha-missing");
                assert_eq!(second_name, "beta-missing");
                assert_ne!(first_name, second_name);
            }
            other => panic!("expected two TemplateError::NotFound results, got {other:?}"),
        }
    }

    #[test]
    fn test_register_template_syntax_error_name_matches_registered_name() {
        let mut engine = HandlebarsTemplateEngine::new();

        let result = engine.register_template(
            "custom-template-name",
            "{{#each items}}no closing tag",
            vec![],
        );

        match result {
            Err(TemplateError::SyntaxError { name, .. }) => {
                assert_eq!(name, "custom-template-name");
            }
            other => panic!("expected TemplateError::SyntaxError, got {other:?}"),
        }
    }

    #[test]
    fn test_register_template_accepts_str_and_string_name_types() {
        let mut engine = HandlebarsTemplateEngine::new();

        let via_str = engine.register_template("literal-str-name", "a", vec![]);
        let via_owned_string =
            engine.register_template(String::from("owned-string-name"), "b", vec![]);

        assert!(via_str.is_ok());
        assert!(via_owned_string.is_ok());
    }

    #[test]
    fn test_handlebars_template_engine_implements_send_sync() {
        assert_send_sync::<HandlebarsTemplateEngine>();
    }

    #[test]
    fn test_render_different_values_produce_different_output() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "Hello {{name}}!", vec!["name".into()])
            .expect("registration must succeed for valid syntax");

        let rendered_alice = engine
            .render("greet", ctx(&[("name", "Alice")]))
            .expect("render must succeed");
        let rendered_bob = engine
            .render("greet", ctx(&[("name", "Bob")]))
            .expect("render must succeed");

        assert_ne!(
            rendered_alice, rendered_bob,
            "a stub returning a constant string would make these equal"
        );
        assert_eq!(rendered_alice, "Hello Alice!");
        assert_eq!(rendered_bob, "Hello Bob!");
    }

    #[test]
    fn test_render_context_value_with_double_brace_syntax_treated_as_literal_text() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("echo", "Value: {{content}}", vec!["content".into()])
            .expect("registration must succeed for valid syntax");

        let rendered = engine
            .render("echo", ctx(&[("content", "{{not_a_var}}")]))
            .expect("render must succeed");

        assert_eq!(
            rendered, "Value: {{not_a_var}}",
            "a context value containing handlebars-like syntax must never be re-evaluated"
        );
    }

    #[test]
    fn test_render_context_value_with_html_special_characters_is_not_html_escaped() {
        // handlebars::Handlebars defaults to HTML-escaping substituted values
        // (rewriting `&`, `<`, `>`, `"` to entities) because the crate was
        // designed primarily for HTML templating. This engine's documented
        // purpose is rendering GitHub Markdown PR bodies and issue comments —
        // contexts where those characters are common and entity-encoding
        // them corrupts the rendered Markdown. `new()` must therefore
        // disable escaping on construction.
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("echo-raw", "{{value}}", vec!["value".to_string()])
            .expect("registration must succeed for valid syntax");

        let rendered = engine
            .render("echo-raw", ctx(&[("value", r#"<a> & "b""#)]))
            .expect("render must succeed when the required variable is present");

        assert_eq!(
            rendered, r#"<a> & "b""#,
            "special characters must be substituted literally, not HTML-entity-encoded"
        );
    }

    #[test]
    fn test_render_unregistered_partial_returns_constraint_violation() {
        // Partial references (`{{> name}}`) are resolved at render time, not
        // at registration time, so `register_template` happily accepts a
        // template body that refers to a partial that was never registered.
        // This exercises the handlebars-runtime-error branch in `render`
        // (distinct from the empty-rendered-output branch exercised by the
        // other `ConstraintViolation` tests) to confirm it is mapped to
        // `ConstraintViolation` rather than panicking or leaking a different
        // error variant.
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("uses-partial", "{{> missing_partial}}", vec![])
            .expect("registration must succeed: partial refs are not validated at register time");

        let result = engine.render("uses-partial", HashMap::new());

        match result {
            Err(TemplateError::ConstraintViolation { name, message }) => {
                assert_eq!(name, "uses-partial");
                assert!(
                    !message.is_empty(),
                    "runtime handlebars error message must not be empty"
                );
            }
            other => panic!("expected TemplateError::ConstraintViolation, got {other:?}"),
        }
    }

    #[test]
    fn test_engine_usable_as_boxed_trait_object() {
        let mut engine = HandlebarsTemplateEngine::new();
        engine
            .register_template("greet", "Hello {{name}}!", vec!["name".into()])
            .expect("registration must succeed for valid syntax");

        let boxed: Box<dyn TemplateEngine> = Box::new(engine);

        let required = boxed
            .list_required_variables("greet")
            .expect("template is registered");
        let rendered = boxed
            .render("greet", ctx(&[("name", "Trait-Object")]))
            .expect("render through the trait object must succeed");

        assert_eq!(required, vec!["name".to_string()]);
        assert_eq!(rendered, "Hello Trait-Object!");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Tier 3: Property-based tests
    // ═══════════════════════════════════════════════════════════════════════

    proptest! {
        /// For any safe alphanumeric value, substituting it into a
        /// single-variable template must reproduce that value literally in
        /// the rendered output at the expected position.
        #[test]
        fn prop_render_substitutes_arbitrary_string_values_literally(
            value in "[a-zA-Z0-9]{1,15}"
        ) {
            let mut engine = HandlebarsTemplateEngine::new();
            engine
                .register_template("greet", "Hello {{name}}!", vec!["name".into()])
                .expect("registration must succeed for valid syntax");

            let rendered = engine
                .render("greet", ctx(&[("name", &value)]))
                .expect("render must succeed for any non-empty required variable value");

            prop_assert_eq!(rendered, format!("Hello {value}!"));
        }

        /// Rendering must never panic regardless of the byte content of a
        /// supplied context value, as long as the required variable is
        /// present — this is pure in-memory string interpolation, not a
        /// parser of untrusted template syntax.
        #[test]
        fn prop_render_never_panics_for_arbitrary_context_values(
            value in ".*"
        ) {
            let mut engine = HandlebarsTemplateEngine::new();
            engine
                .register_template("echo", "X:{{content}}", vec!["content".into()])
                .expect("registration must succeed for valid syntax");

            // The literal "X:" prefix guarantees output is never empty, so
            // this must always succeed — never panic, never a spurious
            // ConstraintViolation.
            let result = engine.render("echo", ctx(&[("content", &value)]));

            prop_assert!(result.is_ok());
        }

        /// `list_required_variables` must echo back exactly the set of
        /// variable names declared at registration — no additions, no
        /// omissions, regardless of how many variables were declared.
        #[test]
        fn prop_list_required_variables_matches_registered_required_vars_exactly(
            names in prop::collection::hash_set("[a-z]{1,10}", 0..8)
        ) {
            let mut engine = HandlebarsTemplateEngine::new();
            let required: Vec<String> = names.iter().cloned().collect();
            // Body does not need to reference every declared variable —
            // required_vars is a pre-render validation gate, not a
            // template-usage check.
            engine
                .register_template("dynamic", "static body", required.clone())
                .expect("registration must succeed for valid syntax");

            let mut returned = engine
                .list_required_variables("dynamic")
                .expect("template is registered");
            let mut expected = required;
            returned.sort();
            expected.sort();

            prop_assert_eq!(returned, expected);
        }

        /// Substitution must occur strictly between the surrounding literal
        /// template text, regardless of the substituted value's content.
        #[test]
        fn prop_render_output_preserves_literal_template_affixes(
            value in "[a-zA-Z0-9]{1,15}"
        ) {
            let mut engine = HandlebarsTemplateEngine::new();
            engine
                .register_template("wrapped", "PREFIX-{{x}}-SUFFIX", vec!["x".into()])
                .expect("registration must succeed for valid syntax");

            let rendered = engine
                .render("wrapped", ctx(&[("x", &value)]))
                .expect("render must succeed");

            prop_assert!(rendered.starts_with("PREFIX-"));
            prop_assert!(rendered.ends_with("-SUFFIX"));
            prop_assert_eq!(rendered, format!("PREFIX-{value}-SUFFIX"));
        }
    }
}
