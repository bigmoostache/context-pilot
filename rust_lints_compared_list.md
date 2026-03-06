# Rust Compiler Lint Configuration — Non-Forbid Lints

> **248** known rustc lints — **204** at forbid, **44** shown below

| Lint | Default | Current | | Irrelevant |
|------|---------|---------|---|---|
| `async_idents` | 💤 allow | 💤 allow |   | 🛑 alias of keyword_idents_2018 |
| `bare_trait_object` | ⚠️ warn | ⚠️ warn |   | 🛑 alias of bare_trait_objects |
| `bindings_with_variant_name` | 🚫 deny | 🚫 deny |  ⚓ |  |
| `dead_code` | ⚠️ warn | 🚫 deny | 🔄 ⚓ |  |
| `default_overrides_default_fields` | 🚫 deny | 🚫 deny |   |  |
| `disjoint_capture_migration` | 💤 allow | 💤 allow |   | 🛑 alias of rust_2021_incompatible_closure_captures |
| `elided_lifetime_in_path` | 💤 allow | 💤 allow |   | 🛑 alias of elided_lifetimes_in_paths |
| `exceeding_bitshifts` | 🚫 deny | 🚫 deny |   | 🛑 alias of arithmetic_overflow |
| `ffi_unwind_calls` | 💤 allow | 💤 allow |   | 🛑 no direct FFI usage |
| `fuzzy_provenance_casts` | 💤 allow | 💤 allow |   | 🛑 strict provenance experimental |
| `inline_always_mismatching_target_features` | ⚠️ warn | ⚠️ warn |   | 🛑 unknown on current rustc |
| `keyword_idents` | 💤 allow | 💤 allow |   | 🛑 alias of keyword_idents_2018 |
| `keyword_idents_2018` | 💤 allow | 💤 allow |   | 🛑 already on edition 2024 |
| `linker_messages` | 💤 allow | 💤 allow |   | 🛑 platform-dependent noise |
| `lossy_provenance_casts` | 💤 allow | 💤 allow |   | 🛑 strict provenance experimental |
| `multiple_supertrait_upcastable` | 💤 allow | 💤 allow |   | 🛑 nightly only |
| `must_not_suspend` | 💤 allow | 💤 allow |   | 🛑 nightly only |
| `non_camel_case_types` | ⚠️ warn | 🚫 deny | 🔄 ⚓ |  |
| `non_exhaustive_omitted_patterns` | 💤 allow | 💤 allow |   | 🛑 noisy with external types |
| `non_fmt_panic` | ⚠️ warn | ⚠️ warn |   | 🛑 alias of non_fmt_panics |
| `non_upper_case_globals` | ⚠️ warn | 🚫 deny | 🔄 ⚓ |  |
| `or_patterns_back_compat` | 💤 allow | 💤 allow |   | 🛑 alias of rust_2021_incompatible_or_patterns |
| `overlapping_patterns` | ⚠️ warn | ⚠️ warn |   | 🛑 alias of overlapping_range_endpoints |
| `private_macro_use` | 🚫 deny | 🚫 deny |   |  |
| `redundant_semicolon` | ⚠️ warn | ⚠️ warn |   | 🛑 alias of redundant_semicolons |
| `resolving_to_items_shadowing_supertrait_items` | 💤 allow | 💤 allow |   | 🛑 future edition prep |
| `rust_2021_incompatible_closure_captures` | 💤 allow | 💤 allow |   | 🛑 already on edition 2024 |
| `rust_2021_incompatible_or_patterns` | 💤 allow | 💤 allow |   | 🛑 already on edition 2024 |
| `rust_2021_prefixes_incompatible_syntax` | 💤 allow | 💤 allow |   | 🛑 already on edition 2024 |
| `rust_2021_prelude_collisions` | 💤 allow | 💤 allow |   | 🛑 already on edition 2024 |
| `shadowing_supertrait_items` | 💤 allow | 💤 allow |   | 🛑 future edition prep |
| `single_use_lifetime` | 💤 allow | 💤 allow |   | 🛑 alias of single_use_lifetimes |
| `static_mut_ref` | ⚠️ warn | ⚠️ warn |   | 🛑 alias of static_mut_refs |
| `tail_call_track_caller` | ⚠️ warn | ⚠️ warn |   | 🛑 unknown on current rustc |
| `test_unstable_lint` | 🚫 deny | 🚫 deny |   |  |
| `unqualified_local_imports` | 💤 allow | 💤 allow |   | 🛑 unstable (rust#138299) |
| `unstable_features` | 💤 allow | 💤 allow |   | 🛑 deprecated lint, does nothing |
| `unstable_name_collision` | ⚠️ warn | ⚠️ warn |   | 🛑 alias of unstable_name_collisions |
| `unsupported_fn_ptr_calling_conventions` | ⚠️ warn | ⚠️ warn |   | 🛑 removed (hard error) |
| `unused_attributes` | ⚠️ warn | 🚫 deny | 🔄 ⚓ |  |
| `unused_doc_comment` | ⚠️ warn | ⚠️ warn |   | 🛑 alias of unused_doc_comments |
| `unused_extern_crates` | 💤 allow | 🚫 deny | 🔄 ⚓ |  |
| `unused_qualifications` | 💤 allow | 🚫 deny | 🔄 ⚓ |  |
| `unused_tuple_struct_fields` | ⚠️ warn | ⚠️ warn |   | 🛑 renamed to dead_code |
