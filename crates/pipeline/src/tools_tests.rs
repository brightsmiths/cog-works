//! Adversarial test suite for `tools.rs` — progressive tool discovery
//! (`build_compact_index`, `search_tools`) and skill validation
//! (`validate_skill_invocation`).
//!
//! ## Phase: RED
//!
//! All three functions under test are `todo!()` stubs. Every test below
//! compiles cleanly but **panics at runtime** until the real implementation
//! lands. This is the expected RED state in TDD.
//!
//! ## Tier map
//!
//! | Target | Tier 1 (spec) | Tier 2 (adversarial) | Tier 3 (proptest) |
//! |---|---|---|---|
//! | `build_compact_index` | 10 | 10 | 4 |
//! | `search_tools` | 6 | 7 | 3 |
//! | `validate_skill_invocation` | 17 | 5 | 3 |
//!
//! ## Assertions covered
//!
//! - **ASSERT-DISC-003**: skills rank above raw tools in search —
//!   `test_search_tools_preserves_skill_first_order_from_build_compact_index_on_matching_query`.
//! - **ASSERT-SKILL-002** `[security]`: only `Active` skills are invocable —
//!   the four `test_validate_skill_invocation_*_lifecycle_rejected_with_lifecycle_inactive`
//!   tests.
//! - **ASSERT-SKILL-005** `[security]`: parameter schema validation rejects
//!   invocations missing required parameters —
//!   `test_validate_skill_invocation_missing_required_parameter_rejected` and
//!   `prop_validate_skill_invocation_missing_any_required_property_always_rejected`.
//!
//! ## Spec interpretation notes
//!
//! - `SkillError::LifecycleInactive::lifecycle_state` is documented as
//!   "serialised as its variant name"; this suite pins that down to the exact
//!   Rust variant identifier (`"Proposed"`, `"Reviewed"`, `"Deprecated"`,
//!   `"Retired"`) rather than a lower-case or `Debug`-formatted-with-fields
//!   rendering. If the intended rendering differs (e.g. includes the
//!   `Deprecated { alternative }` payload), the corresponding test will need
//!   architect clarification — flagged as a gap in the task report.
//! - The hand-rolled JSON Schema subset's behaviour when `required`/
//!   `properties` are present but `type` is absent, and the instance is not
//!   an object, is unspecified by the injected contract; this suite does not
//!   exercise that combination to avoid locking in an unconfirmed assumption.
//! - `SchemaValidationFailed::description` content (beyond non-emptiness) is
//!   only loosely pinned: `test_validate_skill_invocation_schema_failure_description_identifies_missing_property`
//!   asserts the missing property name appears somewhere in the message, on
//!   the basis that "a structured error identifying the problem" (per
//!   ASSERT-SKILL-005) implies naming the offending field. Flagged as a
//!   soft assumption in the task report.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::collections::HashSet;

use proptest::prelude::*;
use serde_json::json;

use super::{
    CompactToolIndex, OutputSchema, SkillError, SkillInvocation, SkillLifecycle, SkillManifest,
    ToolIndexEntry, ValidatedSkillInvocation, build_compact_index, search_tools,
    validate_skill_invocation,
};
use crate::identifiers::{ProfileName, SkillName, ToolName};
use crate::knowledge::{ScopeParameters, ToolProfile};

// ─── Test helpers ────────────────────────────────────────────────────────────

fn tn(s: &str) -> ToolName {
    ToolName::new(s).expect("test tool name must be non-empty")
}

fn tns(names: &[&str]) -> Vec<ToolName> {
    names.iter().copied().map(tn).collect()
}

fn sn(s: &str) -> SkillName {
    SkillName::new(s).expect("test skill name must be non-empty")
}

fn sns(names: &[&str]) -> Vec<SkillName> {
    names.iter().copied().map(sn).collect()
}

fn pn(s: &str) -> ProfileName {
    ProfileName::new(s).expect("test profile name must be non-empty")
}

fn empty_scope() -> ScopeParameters {
    ScopeParameters {
        max_file_changes: None,
        allowed_artifact_patterns: vec![],
        prohibited_artifact_patterns: vec![],
        max_new_files: 0,
    }
}

fn profile(allowed_tools: &[&str], allowed_skills: &[&str]) -> ToolProfile {
    ToolProfile {
        name: pn("test-profile"),
        node_id: None,
        allowed_tools: tns(allowed_tools),
        allowed_skills: sns(allowed_skills),
        scope_parameters: empty_scope(),
    }
}

fn schema_val(value: serde_json::Value) -> OutputSchema {
    OutputSchema::new(value).expect("test schema root must be a JSON object")
}

fn manifest(
    name: SkillName,
    lifecycle: SkillLifecycle,
    schema: OutputSchema,
    tool_call_sequence: &[&str],
) -> SkillManifest {
    SkillManifest {
        name,
        lifecycle,
        parameter_schema: schema,
        tool_call_sequence: tns(tool_call_sequence),
        scope_constraints: empty_scope(),
    }
}

fn invocation(skill_name: SkillName, parameters: serde_json::Value) -> SkillInvocation {
    SkillInvocation {
        skill_name,
        parameters,
    }
}

/// Permissive schema object with no keywords at all — matches anything.
fn permissive_schema() -> OutputSchema {
    schema_val(json!({}))
}

fn entry(name: &str, description: &str, is_skill: bool) -> ToolIndexEntry {
    ToolIndexEntry {
        tool_name: tn(name),
        description: description.to_string(),
        is_skill,
    }
}

fn index_of(entries: Vec<ToolIndexEntry>) -> CompactToolIndex {
    CompactToolIndex { entries }
}

fn entry_tuple(e: &ToolIndexEntry) -> (String, String, bool) {
    (
        e.tool_name.as_str().to_string(),
        e.description.clone(),
        e.is_skill,
    )
}

fn entries_tuples(entries: &[ToolIndexEntry]) -> Vec<(String, String, bool)> {
    entries.iter().map(entry_tuple).collect()
}

fn assert_lifecycle_inactive(
    result: &Result<ValidatedSkillInvocation, SkillError>,
    expected_skill: &SkillName,
    expected_state: &str,
) {
    match result {
        Err(SkillError::LifecycleInactive {
            skill_name,
            lifecycle_state,
        }) => {
            assert_eq!(skill_name, expected_skill);
            assert_eq!(lifecycle_state, expected_state);
        }
        other => panic!("expected Err(LifecycleInactive), got {other:?}"),
    }
}

fn assert_profile_prohibited(
    result: &Result<ValidatedSkillInvocation, SkillError>,
    expected_skill: &SkillName,
) {
    match result {
        Err(SkillError::ProfileProhibited { skill_name }) => {
            assert_eq!(skill_name, expected_skill);
        }
        other => panic!("expected Err(ProfileProhibited), got {other:?}"),
    }
}

fn assert_schema_validation_failed(result: &Result<ValidatedSkillInvocation, SkillError>) {
    match result {
        Err(SkillError::SchemaValidationFailed { description }) => {
            assert!(
                !description.is_empty(),
                "SchemaValidationFailed description must not be empty"
            );
        }
        other => panic!("expected Err(SchemaValidationFailed), got {other:?}"),
    }
}

/// Builds a schema requiring `type: "object"`, a `required` list of the
/// given property names, and `properties` declaring each as `type: "string"`.
fn schema_requiring(required: &[&str]) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    for name in required {
        properties.insert((*name).to_string(), json!({"type": "string"}));
    }
    json!({
        "type": "object",
        "required": required,
        "properties": properties,
    })
}

/// Builds a JSON object schema nested `depth` levels deep via a single
/// `"nested"` property chain, terminating in a `{"type": "string"}` leaf.
/// Built iteratively (a `for` loop, never recursion) so the test fixture
/// builder itself cannot be the thing that overflows the stack.
fn deep_object_schema(depth: usize) -> serde_json::Value {
    let mut schema = json!({"type": "string"});
    for _ in 0..depth {
        schema = json!({
            "type": "object",
            "properties": { "nested": schema },
        });
    }
    schema
}

/// Builds a plain (non-schema) JSON value nested `depth` levels deep via a
/// single `"nested"` key chain. Built iteratively.
fn deep_nested_value(depth: usize) -> serde_json::Value {
    let mut value = json!({"leaf": true});
    for _ in 0..depth {
        value = json!({ "nested": value });
    }
    value
}

/// Builds a matching (schema, parameters) pair nested `depth` levels deep,
/// where each level requires its `"nested"` child and the leaf is a string.
/// Built iteratively so schema and params are always mutually conforming.
fn nested_schema_and_params(depth: usize) -> (serde_json::Value, serde_json::Value) {
    let mut schema = json!({"type": "string"});
    let mut params = json!("leaf-value");
    for _ in 0..depth {
        schema = json!({
            "type": "object",
            "required": ["nested"],
            "properties": { "nested": schema },
        });
        params = json!({ "nested": params });
    }
    (schema, params)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification tests — build_compact_index
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_build_compact_index_empty_tool_list_returns_empty_entries() {
    let index = build_compact_index(&[], &[]);
    assert!(index.entries.is_empty());
}

#[test]
fn test_build_compact_index_empty_profiles_marks_all_tools_as_not_skill() {
    let tool_list = tns(&["run-tests", "lint"]);
    let index = build_compact_index(&tool_list, &[]);

    assert!(index.entries.iter().all(|e| !e.is_skill));
}

#[test]
fn test_build_compact_index_tool_name_present_in_profile_allowed_skills_marked_is_skill_true() {
    let tool_list = tns(&["validate-rust-module"]);
    let profiles = vec![profile(&[], &["validate-rust-module"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert_eq!(index.entries.len(), 1);
    assert!(index.entries[0].is_skill);
}

#[test]
fn test_build_compact_index_tool_name_absent_from_allowed_skills_marked_is_skill_false() {
    let tool_list = tns(&["raw-file-read"]);
    let profiles = vec![profile(&[], &["unrelated-skill"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert_eq!(index.entries.len(), 1);
    assert!(!index.entries[0].is_skill);
}

#[test]
fn test_build_compact_index_description_is_always_empty_string() {
    let tool_list = tns(&["a", "b"]);
    let profiles = vec![profile(&[], &["a"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert!(index.entries.iter().all(|e| e.description.is_empty()));
}

#[test]
fn test_build_compact_index_skills_ranked_before_raw_tools() {
    let tool_list = tns(&["zzz-raw-tool", "aaa-skill"]);
    let profiles = vec![profile(&[], &["aaa-skill"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert_eq!(index.entries.len(), 2);
    assert!(
        index.entries[0].is_skill,
        "skill must appear first despite alphabetically sorting last"
    );
    assert_eq!(index.entries[0].tool_name, tn("aaa-skill"));
    assert!(!index.entries[1].is_skill);
    assert_eq!(index.entries[1].tool_name, tn("zzz-raw-tool"));
}

#[test]
fn test_build_compact_index_skills_sorted_alphabetically_within_group() {
    let tool_list = tns(&["zebra-skill", "apple-skill", "mango-skill"]);
    let profiles = vec![profile(&[], &["zebra-skill", "apple-skill", "mango-skill"])];

    let index = build_compact_index(&tool_list, &profiles);

    let names: Vec<&str> = index.entries.iter().map(|e| e.tool_name.as_str()).collect();
    assert_eq!(names, vec!["apple-skill", "mango-skill", "zebra-skill"]);
}

#[test]
fn test_build_compact_index_raw_tools_sorted_alphabetically_within_group() {
    let tool_list = tns(&["zebra-tool", "apple-tool", "mango-tool"]);

    let index = build_compact_index(&tool_list, &[]);

    let names: Vec<&str> = index.entries.iter().map(|e| e.tool_name.as_str()).collect();
    assert_eq!(names, vec!["apple-tool", "mango-tool", "zebra-tool"]);
}

#[test]
fn test_build_compact_index_skill_name_found_in_any_of_multiple_profiles() {
    let tool_list = tns(&["skill-in-second-profile"]);
    let profiles = vec![
        profile(&[], &["unrelated"]),
        profile(&[], &["skill-in-second-profile"]),
    ];

    let index = build_compact_index(&tool_list, &profiles);

    assert_eq!(index.entries.len(), 1);
    assert!(index.entries[0].is_skill);
}

#[test]
fn test_build_compact_index_entry_count_matches_tool_list_length() {
    let tool_list = tns(&["a", "b", "c", "d"]);

    let index = build_compact_index(&tool_list, &[]);

    assert_eq!(index.entries.len(), 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 2: Adversarial / boundary / stub-killing tests — build_compact_index
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_build_compact_index_duplicate_tool_names_both_entries_kept() {
    let tool_list = tns(&["dup", "dup"]);

    let index = build_compact_index(&tool_list, &[]);

    assert_eq!(
        index.entries.len(),
        2,
        "duplicate tool names in tool_list must both be kept, not deduplicated"
    );
}

#[test]
fn test_build_compact_index_unsorted_input_order_still_produces_sorted_output() {
    let tool_list = tns(&["zzz", "mmm", "aaa"]);

    let index = build_compact_index(&tool_list, &[]);

    let names: Vec<&str> = index.entries.iter().map(|e| e.tool_name.as_str()).collect();
    assert_eq!(names, vec!["aaa", "mmm", "zzz"]);
}

#[test]
fn test_build_compact_index_tool_name_substring_of_skill_name_not_marked_as_skill() {
    // "val" is a substring of "validate-rust-module" but must not match it —
    // the comparison is by exact underlying string equality, not substring.
    let tool_list = tns(&["val"]);
    let profiles = vec![profile(&[], &["validate-rust-module"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert_eq!(index.entries.len(), 1);
    assert!(
        !index.entries[0].is_skill,
        "a tool name that is merely a substring of a skill name must not be marked is_skill"
    );
}

#[test]
fn test_build_compact_index_match_is_case_sensitive() {
    let tool_list = tns(&["Build"]);
    let profiles = vec![profile(&[], &["build"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert_eq!(index.entries.len(), 1);
    assert!(
        !index.entries[0].is_skill,
        "skill-name matching must be case-sensitive string equality"
    );
}

#[test]
fn test_build_compact_index_profile_with_unrelated_skills_does_not_mark_tool_as_skill() {
    let tool_list = tns(&["my-tool"]);
    let profiles = vec![profile(&[], &["other-skill-1", "other-skill-2"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert!(!index.entries[0].is_skill);
}

#[test]
fn test_build_compact_index_single_tool_single_matching_skill() {
    let tool_list = tns(&["only-tool"]);
    let profiles = vec![profile(&[], &["only-tool"])];

    let index = build_compact_index(&tool_list, &profiles);

    assert_eq!(index.entries.len(), 1);
    assert!(index.entries[0].is_skill);
}

#[test]
fn test_build_compact_index_all_tools_are_skills_sorted_alphabetically_only() {
    let tool_list = tns(&["c-skill", "a-skill", "b-skill"]);
    let profiles = vec![profile(&[], &["c-skill", "a-skill", "b-skill"])];

    let index = build_compact_index(&tool_list, &profiles);

    let names: Vec<&str> = index.entries.iter().map(|e| e.tool_name.as_str()).collect();
    assert_eq!(names, vec!["a-skill", "b-skill", "c-skill"]);
    assert!(index.entries.iter().all(|e| e.is_skill));
}

#[test]
fn test_build_compact_index_stub_cannot_return_empty_entries_for_nonempty_input() {
    let tool_list = tns(&["one", "two", "three"]);

    let index = build_compact_index(&tool_list, &[]);

    assert_eq!(
        index.entries.len(),
        3,
        "a stub returning an unconditionally empty index must fail this test"
    );
}

#[test]
fn test_build_compact_index_stub_cannot_mark_everything_as_skill() {
    let tool_list = tns(&["plain-tool"]);

    let index = build_compact_index(&tool_list, &[]);

    assert!(
        !index.entries[0].is_skill,
        "a stub that unconditionally marks every entry as a skill must fail this test"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 3: Property-based tests — build_compact_index
// ═══════════════════════════════════════════════════════════════════════════

fn arbitrary_name() -> impl Strategy<Value = String> {
    "[a-z]{2,6}"
}

fn arbitrary_tool_list_and_profiles() -> impl Strategy<Value = (Vec<String>, Vec<Vec<String>>)> {
    (
        prop::collection::vec(arbitrary_name(), 0..8),
        prop::collection::vec(prop::collection::vec(arbitrary_name(), 0..4), 0..3),
    )
}

fn expected_build(
    tool_names: &[String],
    profile_skill_lists: &[Vec<String>],
) -> Vec<(String, String, bool)> {
    let union: HashSet<String> = profile_skill_lists.iter().flatten().cloned().collect();
    let mut rows: Vec<(String, String, bool)> = tool_names
        .iter()
        .map(|name| (name.clone(), String::new(), union.contains(name)))
        .collect();
    rows.sort_by(|a, b| (!a.2, a.0.clone()).cmp(&(!b.2, b.0.clone())));
    rows
}

proptest! {
    #[test]
    fn prop_build_compact_index_entry_count_equals_input_length(
        (tool_names, _profile_skill_lists) in arbitrary_tool_list_and_profiles(),
    ) {
        let tool_list: Vec<ToolName> = tool_names.iter().map(|n| tn(n)).collect();
        let index = build_compact_index(&tool_list, &[]);
        prop_assert_eq!(index.entries.len(), tool_list.len());
    }

    #[test]
    fn prop_build_compact_index_matches_independent_oracle(
        (tool_names, profile_skill_lists) in arbitrary_tool_list_and_profiles(),
    ) {
        let tool_list: Vec<ToolName> = tool_names.iter().map(|n| tn(n)).collect();
        let profiles: Vec<ToolProfile> = profile_skill_lists
            .iter()
            .map(|skills| {
                let skill_refs: Vec<&str> = skills.iter().map(String::as_str).collect();
                profile(&[], &skill_refs)
            })
            .collect();

        let index = build_compact_index(&tool_list, &profiles);
        let expected = expected_build(&tool_names, &profile_skill_lists);

        prop_assert_eq!(entries_tuples(&index.entries), expected);
    }

    #[test]
    fn prop_build_compact_index_output_is_sorted_skills_first_then_alphabetical(
        (tool_names, profile_skill_lists) in arbitrary_tool_list_and_profiles(),
    ) {
        let tool_list: Vec<ToolName> = tool_names.iter().map(|n| tn(n)).collect();
        let profiles: Vec<ToolProfile> = profile_skill_lists
            .iter()
            .map(|skills| {
                let skill_refs: Vec<&str> = skills.iter().map(String::as_str).collect();
                profile(&[], &skill_refs)
            })
            .collect();

        let index = build_compact_index(&tool_list, &profiles);

        let sorted = index.entries.windows(2).all(|w| {
            let a = (!w[0].is_skill, w[0].tool_name.as_str());
            let b = (!w[1].is_skill, w[1].tool_name.as_str());
            a <= b
        });
        prop_assert!(sorted);
    }

    #[test]
    fn prop_build_compact_index_description_always_empty(
        (tool_names, profile_skill_lists) in arbitrary_tool_list_and_profiles(),
    ) {
        let tool_list: Vec<ToolName> = tool_names.iter().map(|n| tn(n)).collect();
        let profiles: Vec<ToolProfile> = profile_skill_lists
            .iter()
            .map(|skills| {
                let skill_refs: Vec<&str> = skills.iter().map(String::as_str).collect();
                profile(&[], &skill_refs)
            })
            .collect();

        let index = build_compact_index(&tool_list, &profiles);

        prop_assert!(index.entries.iter().all(|e| e.description.is_empty()));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification tests — search_tools
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_search_tools_empty_query_returns_all_entries_in_order() {
    let index = index_of(vec![entry("skill-a", "", true), entry("tool-b", "", false)]);

    let results = search_tools(&index, "");

    assert_eq!(entries_tuples(&results), entries_tuples(&index.entries));
}

#[test]
fn test_search_tools_query_matches_tool_name_only() {
    let index = index_of(vec![
        entry("validate-rust-module", "", false),
        entry("other-tool", "some description", false),
    ]);

    let results = search_tools(&index, "validate");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool_name, tn("validate-rust-module"));
}

#[test]
fn test_search_tools_query_matches_description_only() {
    let index = index_of(vec![
        entry("alpha", "runs the unit tests", false),
        entry("beta", "", false),
    ]);

    let results = search_tools(&index, "unit tests");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool_name, tn("alpha"));
}

#[test]
fn test_search_tools_no_match_returns_empty_vec() {
    let index = index_of(vec![entry("alpha", "description", false)]);

    let results = search_tools(&index, "zzz-nonexistent-zzz");

    assert!(results.is_empty());
}

#[test]
fn test_search_tools_case_insensitive_match_uppercase_query() {
    let index = index_of(vec![entry("validate-rust-module", "", false)]);

    let results = search_tools(&index, "VALIDATE");

    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_tools_preserves_relative_order_of_index() {
    let index = index_of(vec![
        entry("test-skill-one", "", true),
        entry("test-skill-two", "", true),
        entry("test-tool-three", "", false),
    ]);

    let results = search_tools(&index, "test");

    let names: Vec<&str> = results.iter().map(|e| e.tool_name.as_str()).collect();
    assert_eq!(
        names,
        vec!["test-skill-one", "test-skill-two", "test-tool-three"]
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 2: Adversarial / boundary / stub-killing tests — search_tools
// ═══════════════════════════════════════════════════════════════════════════

/// ASSERT-DISC-003: a query matching both a skill and a raw tool must return
/// the skill before the raw tool.
///
/// `search_tools` itself is a pure order-preserving filter (per
/// `docs/spec/interfaces/advanced-features.md` §search_tools, the "skills
/// before raw tools" parenthetical describes the order `index.entries` is
/// already expected to be in, not an instruction for `search_tools` to
/// re-sort). The skill-first ordering guarantee is established by
/// `build_compact_index`. This test therefore exercises the real end-to-end
/// invariant: build the index via `build_compact_index` from a `tool_list`
/// containing both a raw tool and a skill name, with a `ToolProfile` whose
/// `allowed_skills` includes the skill — then confirm `search_tools`
/// preserves that skill-first order on a query matching both.
#[test]
fn test_search_tools_preserves_skill_first_order_from_build_compact_index_on_matching_query() {
    let tool_list = tns(&["deploy-raw-tool", "deploy-skill"]);
    let profiles = vec![profile(&[], &["deploy-skill"])];
    let index = build_compact_index(&tool_list, &profiles);

    let results = search_tools(&index, "deploy");

    assert_eq!(results.len(), 2);
    assert!(
        results[0].is_skill,
        "skill must be returned before the raw tool"
    );
    assert_eq!(results[0].tool_name, tn("deploy-skill"));
    assert!(!results[1].is_skill);
    assert_eq!(results[1].tool_name, tn("deploy-raw-tool"));
}

#[test]
fn test_search_tools_partial_substring_match() {
    let index = index_of(vec![entry("validate-rust-module", "", false)]);

    let results = search_tools(&index, "rust-mod");

    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_tools_mixed_case_description_match() {
    let index = index_of(vec![entry("alpha", "Runs Unit Tests Quickly", false)]);

    let results = search_tools(&index, "unit tests");

    assert_eq!(results.len(), 1);
}

#[test]
fn test_search_tools_empty_index_returns_empty_regardless_of_query() {
    let index = index_of(vec![]);

    assert!(search_tools(&index, "").is_empty());
    assert!(search_tools(&index, "anything").is_empty());
}

#[test]
fn test_search_tools_does_not_mutate_input_index() {
    let index = index_of(vec![
        entry("alpha", "desc", false),
        entry("beta", "desc", true),
    ]);
    let before = entries_tuples(&index.entries);

    let _ = search_tools(&index, "alpha");

    assert_eq!(entries_tuples(&index.entries), before);
}

#[test]
fn test_search_tools_stub_cannot_return_full_list_for_nonmatching_query() {
    let index = index_of(vec![entry("alpha", "", false), entry("beta", "", false)]);

    let results = search_tools(&index, "zzz-does-not-exist-zzz");

    assert!(
        results.is_empty(),
        "a stub that always returns all entries must fail this test"
    );
}

#[test]
fn test_search_tools_stub_cannot_return_empty_for_matching_query() {
    let index = index_of(vec![entry("alpha", "", false)]);

    let results = search_tools(&index, "alpha");

    assert!(
        !results.is_empty(),
        "a stub that always returns an empty vec must fail this test"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 3: Property-based tests — search_tools
// ═══════════════════════════════════════════════════════════════════════════

fn arbitrary_entry() -> impl Strategy<Value = ToolIndexEntry> {
    ("[a-zA-Z0-9_-]{1,10}", "[a-zA-Z0-9 _-]{0,15}", any::<bool>())
        .prop_map(|(name, desc, is_skill)| entry(&name, &desc, is_skill))
}

fn arbitrary_index() -> impl Strategy<Value = CompactToolIndex> {
    prop::collection::vec(arbitrary_entry(), 0..10).prop_map(index_of)
}

fn expected_search(index: &CompactToolIndex, query: &str) -> Vec<(String, String, bool)> {
    let q = query.to_lowercase();
    entries_tuples(&index.entries)
        .into_iter()
        .filter(|(name, desc, _)| {
            name.to_lowercase().contains(&q) || desc.to_lowercase().contains(&q)
        })
        .collect()
}

proptest! {
    /// Oracle test: re-derives the expected filtered-and-order-preserved
    /// result independently of the implementation and compares directly.
    #[test]
    fn prop_search_tools_matches_independent_oracle(
        index in arbitrary_index(),
        query in "[a-zA-Z0-9 _-]{0,6}",
    ) {
        let results = search_tools(&index, &query);
        let expected = expected_search(&index, &query);
        prop_assert_eq!(entries_tuples(&results), expected);
    }

    #[test]
    fn prop_search_tools_empty_query_always_returns_full_length(
        index in arbitrary_index(),
    ) {
        let results = search_tools(&index, "");
        prop_assert_eq!(results.len(), index.entries.len());
    }

    #[test]
    fn prop_search_tools_case_insensitivity_upper_and_lower_query_yield_same_results(
        index in arbitrary_index(),
        query in "[a-zA-Z]{1,6}",
    ) {
        let lower = search_tools(&index, &query.to_lowercase());
        let upper = search_tools(&index, &query.to_uppercase());
        prop_assert_eq!(entries_tuples(&lower), entries_tuples(&upper));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification tests — validate_skill_invocation (lifecycle gate)
// ASSERT-SKILL-002
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_validate_skill_invocation_proposed_lifecycle_rejected_with_lifecycle_inactive() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Proposed,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_lifecycle_inactive(&result, &skill, "Proposed");
}

#[test]
fn test_validate_skill_invocation_reviewed_lifecycle_rejected_with_lifecycle_inactive() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Reviewed,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_lifecycle_inactive(&result, &skill, "Reviewed");
}

#[test]
fn test_validate_skill_invocation_deprecated_lifecycle_rejected_with_lifecycle_inactive() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Deprecated {
            alternative: sn("my-skill-v2"),
        },
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_lifecycle_inactive(&result, &skill, "Deprecated");
}

#[test]
fn test_validate_skill_invocation_retired_lifecycle_rejected_with_lifecycle_inactive() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Retired,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_lifecycle_inactive(&result, &skill, "Retired");
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification tests — validate_skill_invocation (profile gate)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_validate_skill_invocation_skill_not_present_in_profile_allowed_skills_rejected() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &[]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_profile_prohibited(&result, &skill);
}

#[test]
fn test_validate_skill_invocation_profile_has_other_skills_but_not_this_one_rejected() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["other-skill-1", "other-skill-2"]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_profile_prohibited(&result, &skill);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 1: Specification tests — validate_skill_invocation (schema gate)
// ASSERT-SKILL-005
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_validate_skill_invocation_missing_required_parameter_rejected() {
    let skill = sn("my-skill");
    let schema = schema_val(schema_requiring(&["repo"]));
    let m = manifest(skill.clone(), SkillLifecycle::Active, schema, &[]);
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_schema_validation_failed(&result);
}

#[test]
fn test_validate_skill_invocation_type_mismatch_string_expected_number_given_rejected() {
    let skill = sn("my-skill");
    let schema = schema_val(schema_requiring(&["repo"]));
    let m = manifest(skill.clone(), SkillLifecycle::Active, schema, &[]);
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"repo": 12345}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_schema_validation_failed(&result);
}

#[test]
fn test_validate_skill_invocation_fully_conforming_invocation_passes_schema_gate_and_succeeds() {
    let skill = sn("my-skill");
    let schema = schema_val(schema_requiring(&["repo"]));
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema,
        &["tool-a", "tool-b"],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"repo": "owner/repo"}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(result.is_ok(), "expected Ok, got {result:?}");
}

#[test]
fn test_validate_skill_invocation_nested_object_missing_required_subfield_rejected() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "properties": {
            "nested": {"type": "object", "required": ["id"]}
        }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"nested": {}}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_schema_validation_failed(&result);
}

#[test]
fn test_validate_skill_invocation_nested_object_conforming_subfield_passes() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "properties": {
            "nested": {"type": "object", "required": ["id"]}
        }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"nested": {"id": "abc"}}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(result.is_ok(), "expected Ok, got {result:?}");
}

#[test]
fn test_validate_skill_invocation_unsupported_pattern_keyword_is_ignored_permissively() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "required": ["code"],
        "properties": {
            "code": {"type": "string", "pattern": "^[A-Z]+$"}
        }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    // Value deliberately violates the (unsupported) `pattern` keyword.
    let inv = invocation(skill, json!({"code": "lowercase-does-not-match-pattern"}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(
        result.is_ok(),
        "unsupported JSON Schema keywords like `pattern` must be silently ignored; got {result:?}"
    );
}

#[test]
fn test_validate_skill_invocation_additional_properties_not_declared_in_schema_allowed() {
    let skill = sn("my-skill");
    let schema = schema_val(schema_requiring(&["name"]));
    let m = manifest(skill.clone(), SkillLifecycle::Active, schema, &[]);
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"name": "x", "extra_unexpected_field": 123}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(
        result.is_ok(),
        "extra properties not declared in the schema must be allowed (no additionalProperties support); got {result:?}"
    );
}

#[test]
fn test_validate_skill_invocation_empty_schema_object_accepts_any_parameters() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"anything": "goes", "number": 42}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(result.is_ok(), "expected Ok, got {result:?}");
}

#[test]
fn test_validate_skill_invocation_integer_type_rejects_fractional_number() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "required": ["count"],
        "properties": { "count": {"type": "integer"} }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"count": 3.5}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_schema_validation_failed(&result);
}

/// Mutation-kill test (survivor: `cargo mutants` mutant
/// `tools.rs:879:5: replace is_integer_value -> bool with false`).
///
/// The suite previously only exercised the *rejection* path for `type:
/// "integer"` (fractional numbers); nothing asserted that an ordinary
/// positive integer is actually *accepted*. A stub that always returns
/// `false` from `is_integer_value` therefore passed every existing test.
#[test]
fn test_validate_skill_invocation_integer_type_accepts_plain_positive_integer() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "required": ["count"],
        "properties": { "count": {"type": "integer"} }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"count": 5}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(
        result.is_ok(),
        "a plain positive integer must satisfy type 'integer'; got {result:?}"
    );
}

/// Mutation-kill test (survivor: `cargo mutants` mutant
/// `tools.rs:881:38: replace || with && in is_integer_value`).
///
/// `is_integer_value` must also accept a JSON number that serde_json stores
/// as a float but which has no fractional part (e.g. `5.0`), via its
/// `as_f64().is_some_and(|f| f.fract() == 0.0)` fallback branch. This is the
/// one case where the three disjuncts are *not* mutually redundant: for a
/// float-encoded whole number, `is_i64()` and `is_u64()` are both `false`,
/// so only the third disjunct can accept it. Flipping the second `||` to
/// `&&` makes this fallback branch require `is_u64()` to also be true,
/// incorrectly rejecting `5.0`.
#[test]
fn test_validate_skill_invocation_integer_type_accepts_whole_number_encoded_as_float() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "required": ["count"],
        "properties": { "count": {"type": "integer"} }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    // `5.0` is a Rust f64 literal; `json!` stores it as a serde_json float
    // Number (is_i64() == false, is_u64() == false) despite being
    // integer-valued.
    let inv = invocation(skill, json!({"count": 5.0}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(
        result.is_ok(),
        "a float-encoded whole number (5.0) must satisfy type 'integer'; got {result:?}"
    );
}

/// Mutation-kill test (survivors: `cargo mutants` mutants
/// `tools.rs:890:5: replace json_type_name -> &'static str with ""` and
/// `with "xyzzy"`).
///
/// `SchemaValidationFailed::description` is documented to identify "the
/// problem"; for a type mismatch this must include the *actual* runtime
/// type found, not just the expected type. No existing test inspected that
/// half of the message, so a stub `json_type_name` returning a constant
/// string for every JSON value passed silently.
#[test]
fn test_validate_skill_invocation_schema_failure_description_identifies_actual_type_found() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "required": ["repo"],
        "properties": { "repo": {"type": "string"} }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"repo": 12345}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    match result {
        Err(SkillError::SchemaValidationFailed { description }) => {
            assert!(
                description.contains("number"),
                "description must identify the actual found type ('number'); got: {description}"
            );
            assert!(
                !description.contains("xyzzy"),
                "description must not contain mutant placeholder text; got: {description}"
            );
        }
        other => panic!("expected SchemaValidationFailed, got {other:?}"),
    }
}

#[test]
fn test_validate_skill_invocation_boolean_type_mismatch_rejected() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "required": ["flag"],
        "properties": { "flag": {"type": "boolean"} }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({"flag": "not-a-bool"}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_schema_validation_failed(&result);
}

#[test]
fn test_validate_skill_invocation_null_type_matches_only_null() {
    let skill = sn("my-skill");
    let schema_value = json!({
        "type": "object",
        "required": ["marker"],
        "properties": { "marker": {"type": "null"} }
    });
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);

    let ok_inv = invocation(skill.clone(), json!({"marker": null}));
    let bad_inv = invocation(skill, json!({"marker": "not-null"}));

    assert!(validate_skill_invocation(&ok_inv, &m, &prof).is_ok());
    assert_schema_validation_failed(&validate_skill_invocation(&bad_inv, &m, &prof));
}

#[test]
fn test_validate_skill_invocation_success_carries_tool_call_sequence_from_manifest() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        permissive_schema(),
        &["tool-one", "tool-two", "tool-three"],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof).expect("expected Ok");

    assert_eq!(result.tool_call_sequence(), m.tool_call_sequence.as_slice());
}

#[test]
fn test_validate_skill_invocation_success_invocation_accessor_returns_original_invocation() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let params = json!({"key": "value"});
    let inv = invocation(skill.clone(), params.clone());

    let result = validate_skill_invocation(&inv, &m, &prof).expect("expected Ok");

    assert_eq!(result.invocation().skill_name, skill);
    assert_eq!(result.invocation().parameters, params);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 2: Adversarial tests — gate ordering / short-circuiting
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_validate_skill_invocation_lifecycle_gate_short_circuits_profile_and_schema_failures() {
    let skill = sn("my-skill");
    // Lifecycle not Active, profile does NOT allow the skill, AND parameters
    // fail schema validation. The lifecycle gate must win.
    let schema = schema_val(schema_requiring(&["repo"]));
    let m = manifest(skill.clone(), SkillLifecycle::Proposed, schema, &[]);
    let prof = profile(&[], &[]); // skill not allowed
    let inv = invocation(skill.clone(), json!({})); // missing required "repo"

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_lifecycle_inactive(&result, &skill, "Proposed");
}

#[test]
fn test_validate_skill_invocation_profile_gate_short_circuits_schema_failure() {
    let skill = sn("my-skill");
    // Lifecycle Active, profile does NOT allow the skill, AND parameters fail
    // schema validation. The profile gate must win (not SchemaValidationFailed).
    let schema = schema_val(schema_requiring(&["repo"]));
    let m = manifest(skill.clone(), SkillLifecycle::Active, schema, &[]);
    let prof = profile(&[], &[]); // skill not allowed
    let inv = invocation(skill.clone(), json!({})); // missing required "repo"

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_profile_prohibited(&result, &skill);
}

#[test]
fn test_validate_skill_invocation_lifecycle_inactive_error_identifies_skill_name() {
    let skill = sn("specific-skill-name");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Retired,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &[]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    match result {
        Err(SkillError::LifecycleInactive { skill_name, .. }) => {
            assert_eq!(skill_name, skill);
        }
        other => panic!("expected LifecycleInactive, got {other:?}"),
    }
}

#[test]
fn test_validate_skill_invocation_profile_prohibited_error_identifies_skill_name() {
    let skill = sn("specific-skill-name");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &[]);
    let inv = invocation(skill.clone(), json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    match result {
        Err(SkillError::ProfileProhibited { skill_name }) => {
            assert_eq!(skill_name, skill);
        }
        other => panic!("expected ProfileProhibited, got {other:?}"),
    }
}

#[test]
fn test_validate_skill_invocation_schema_failure_description_identifies_missing_property() {
    let skill = sn("my-skill");
    let schema = schema_val(schema_requiring(&["repository_identifier"]));
    let m = manifest(skill.clone(), SkillLifecycle::Active, schema, &[]);
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    match result {
        Err(SkillError::SchemaValidationFailed { description }) => {
            assert!(
                description.contains("repository_identifier"),
                "description should identify the missing property; got: {description}"
            );
        }
        other => panic!("expected SchemaValidationFailed, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 3: Adversarial / security tests — recursive depth guard
// ═══════════════════════════════════════════════════════════════════════════

/// Security/robustness test (DoS-hardening precedent: `json_guard.rs`'s
/// `MAX_SCHEMA_COMPARISON_DEPTH` = 64). An adversarially deep
/// `manifest.parameter_schema` must be rejected with
/// `SchemaValidationFailed`, never cause a panic or stack overflow.
#[test]
fn test_validate_skill_invocation_adversarially_deep_schema_rejected_not_panicking() {
    let skill = sn("my-skill");
    let deep_schema = schema_val(deep_object_schema(70));
    let m = manifest(skill.clone(), SkillLifecycle::Active, deep_schema, &[]);
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, json!({}));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_schema_validation_failed(&result);
}

/// Same guard, but the adversarially deep value is the caller-supplied
/// `invocation.parameters` rather than the schema.
#[test]
fn test_validate_skill_invocation_adversarially_deep_parameters_rejected_not_panicking() {
    let skill = sn("my-skill");
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        permissive_schema(),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, deep_nested_value(70));

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert_schema_validation_failed(&result);
}

/// Negative control for the depth guard: a legitimately (moderately) nested
/// schema/parameters pair, well within `MAX_SCHEMA_COMPARISON_DEPTH`, must
/// still validate correctly end-to-end — the guard must not reject
/// reasonable nesting.
#[test]
fn test_validate_skill_invocation_moderately_deep_schema_within_limit_still_validates_correctly() {
    let skill = sn("my-skill");
    let (schema_value, params_value) = nested_schema_and_params(5);
    let m = manifest(
        skill.clone(),
        SkillLifecycle::Active,
        schema_val(schema_value),
        &[],
    );
    let prof = profile(&[], &["my-skill"]);
    let inv = invocation(skill, params_value);

    let result = validate_skill_invocation(&inv, &m, &prof);

    assert!(result.is_ok(), "expected Ok, got {result:?}");
}

// ═══════════════════════════════════════════════════════════════════════════
// Tier 3: Property-based tests — validate_skill_invocation
// ═══════════════════════════════════════════════════════════════════════════

fn arbitrary_inactive_lifecycle() -> impl Strategy<Value = SkillLifecycle> {
    prop_oneof![
        Just(SkillLifecycle::Proposed),
        Just(SkillLifecycle::Reviewed),
        Just(SkillLifecycle::Retired),
        "[a-z]{3,8}".prop_map(|s| SkillLifecycle::Deprecated {
            alternative: sn(&s)
        }),
    ]
}

proptest! {
    /// A non-Active lifecycle must always be rejected with
    /// `LifecycleInactive`, regardless of whether the profile would permit
    /// the skill or whether the parameters would otherwise conform.
    #[test]
    fn prop_validate_skill_invocation_non_active_lifecycle_always_rejected_regardless_of_profile_or_params(
        lifecycle in arbitrary_inactive_lifecycle(),
        profile_allows in any::<bool>(),
        params_conform in any::<bool>(),
    ) {
        let skill = sn("prop-skill");
        let schema = schema_val(schema_requiring(&["repo"]));
        let m = manifest(skill.clone(), lifecycle, schema, &[]);
        let prof = if profile_allows {
            profile(&[], &["prop-skill"])
        } else {
            profile(&[], &[])
        };
        let params = if params_conform {
            json!({"repo": "owner/repo"})
        } else {
            json!({})
        };
        let inv = invocation(skill.clone(), params);

        let result = validate_skill_invocation(&inv, &m, &prof);

        let is_lifecycle_inactive = matches!(result, Err(SkillError::LifecycleInactive { .. }));
        prop_assert!(
            is_lifecycle_inactive,
            "non-Active lifecycle must always be rejected with LifecycleInactive"
        );
    }

    /// When all three gates pass, the result is always `Ok`, and the
    /// resolved tool call sequence always matches the manifest's.
    #[test]
    fn prop_validate_skill_invocation_all_gates_pass_always_returns_ok_with_matching_tool_call_sequence(
        tool_names in prop::collection::vec("[a-z]{2,6}", 0..5),
    ) {
        let skill = sn("prop-skill");
        let tool_refs: Vec<&str> = tool_names.iter().map(String::as_str).collect();
        let m = manifest(skill.clone(), SkillLifecycle::Active, permissive_schema(), &tool_refs);
        let prof = profile(&[], &["prop-skill"]);
        let inv = invocation(skill, json!({}));

        let result = validate_skill_invocation(&inv, &m, &prof);

        match result {
            Ok(validated) => {
                prop_assert_eq!(validated.tool_call_sequence(), m.tool_call_sequence.as_slice());
            }
            Err(e) => prop_assert!(false, "expected Ok, got Err({e:?})"),
        }
    }

    /// Missing any one of a generated set of required properties always
    /// produces `SchemaValidationFailed`; supplying all of them always
    /// succeeds. Generalises ASSERT-SKILL-005 across arbitrary required-set
    /// shapes rather than a single hand-picked example.
    #[test]
    fn prop_validate_skill_invocation_missing_any_required_property_always_rejected(
        n in 1usize..5,
        mask in any::<u8>(),
    ) {
        let names: Vec<String> = (0..n).map(|i| format!("prop{i}")).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let keep: Vec<bool> = (0..n).map(|i| mask & (1 << i) != 0).collect();

        let skill = sn("prop-skill");
        let schema = schema_val(schema_requiring(&name_refs));
        let m = manifest(skill.clone(), SkillLifecycle::Active, schema, &[]);
        let prof = profile(&[], &["prop-skill"]);

        let mut params = serde_json::Map::new();
        for (name, k) in names.iter().zip(keep.iter()) {
            if *k {
                params.insert(name.clone(), json!("value"));
            }
        }
        let inv = invocation(skill, serde_json::Value::Object(params));

        let result = validate_skill_invocation(&inv, &m, &prof);

        if keep.iter().all(|&k| k) {
            prop_assert!(result.is_ok(), "all required properties present: expected Ok, got {result:?}");
        } else {
            prop_assert!(
                matches!(result, Err(SkillError::SchemaValidationFailed { .. })),
                "at least one required property missing: expected SchemaValidationFailed, got {result:?}"
            );
        }
    }
}
