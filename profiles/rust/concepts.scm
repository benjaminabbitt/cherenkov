; Rust concept captures. See profiles/python/concepts.scm for the contract.

(binary_expression left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(unary_expression ["-" "!" "*"] @op (_) @operand) @concept.unary
(compound_assignment_expr left: (_) @target operator: _ @op right: (_) @value) @concept.compound_assign
(assignment_expression left: (_) @target right: (_) @value) @concept.assign
(let_declaration pattern: (_) @target value: (_) @value) @concept.assign

[(boolean_literal) (integer_literal) (float_literal) (string_literal)] @concept.literal

; --- Conditions. `if let` and let chains are not conditions the evaluator
; can negate, so only plain expression kinds are captured.
(if_expression
  condition: [(binary_expression) (identifier) (unary_expression) (call_expression)
              (field_expression) (parenthesized_expression) (boolean_literal)] @cond
  consequence: (block) @then
  alternative: (else_clause)? @else) @concept.condition
(while_expression
  condition: [(binary_expression) (identifier) (unary_expression) (call_expression)
              (field_expression) (parenthesized_expression) (boolean_literal)] @cond
  body: (block) @then) @concept.condition

(return_expression (_)? @value) @concept.return
; A closure is compiled as a function of its own: its body expression is
; its return value.
(closure_expression body: (_) @value @concept.return
  (#not-match? @value "^\\{"))
; A function's tail expression is its return value. Not a last statement
; (ending in `;`) or comment, and not a block-like tail (`if`, `match`,
; `loop`), whose own concepts cover it.
(function_item body: (block
  (_) @value @concept.return .
  (#not-match? @value "^(if|match|loop|while|for|unsafe|\\{|//|/\\*)|;\\s*$")))
(call_expression function: (_) @callee arguments: (_) @args) @concept.call
(index_expression . (_) @base . (_) @index) @concept.index

; `async`: the body runs in the fn's future, whatever code a macro wraps it in.
(function_item (function_modifiers "async" @async)? name: (identifier) @name return_type: (_)? @returns body: (block) @body) @concept.function

; --- Statements. `if`, `while` and `return` are expressions in Rust; the
; expression node is the statement whether or not it ends in `;`.
(expression_statement (call_expression)) @stmt.call
(expression_statement (assignment_expression)) @stmt.assign
(expression_statement (compound_assignment_expr)) @stmt.assign
(let_declaration) @stmt.assign
(return_expression) @stmt.return
(if_expression) @stmt.if
(while_expression) @stmt.loop
(for_expression) @stmt.loop
(loop_expression) @stmt.loop

; --- Match arms. A guarded `_` arm is still the catch-all, as cargo-mutants
; treats it (its pattern is a wildcard).
(match_expression
  body: (match_block
    (match_arm pattern: (match_pattern condition: (_)? @guard) value: (_) @value) @concept.arm)) @scope
(match_arm pattern: (match_pattern "_" @catch_all)) @concept.arm

; --- Struct literal fields: only with a base (`..Default::default()`), which
; supplies the omitted field.
(struct_expression
  body: (field_initializer_list
    [(shorthand_field_initializer) (field_initializer)] @concept.field_init
    (base_field_initializer))) @scope

; --- Skip regions (`@skip.<kind>`): what a set may decline to mutate. An
; outer attribute is a preceding sibling of the item it applies to, perhaps
; with other attributes between; the region is that item. Helper captures
; (`@_name`) only feed predicates.
;
; test: `#[cfg(test)]` (test directly inside cfg(...), as cargo-mutants reads
; it, so `cfg(not(test))` does not count), and any attribute whose path ends
; in `test` (`#[test]`, `#[tokio::test(...)]`).
((attribute_item) @_attr
  .
  (attribute_item)*
  .
  [(function_item) (mod_item) (impl_item) (trait_item) (struct_item) (enum_item)
   (const_item) (static_item) (let_declaration) (expression_statement)
   (field_initializer) (shorthand_field_initializer) (macro_invocation)] @skip.test
  (#match? @_attr "^#\\[\\s*(cfg\\s*\\(\\s*test\\s*\\)|([A-Za-z_][A-Za-z0-9_]*\\s*::\\s*)*test(\\s*\\(.*\\))?)\\s*\\]$"))
((source_file (inner_attribute_item) @_attr) @skip.test
  (#match? @_attr "^#!\\[\\s*cfg\\s*\\(\\s*test\\s*\\)\\s*\\]$"))

; annotated: `#[mutants::skip]`, also through `cfg_attr(..., mutants::skip)`.
((attribute_item) @_attr
  .
  (attribute_item)*
  .
  [(function_item) (mod_item) (impl_item) (trait_item) (let_declaration)
   (expression_statement) (field_initializer) (shorthand_field_initializer)] @skip.annotated
  (#match? @_attr "^#\\[\\s*(mutants\\s*::\\s*skip|cfg_attr\\s*\\(.*\\bmutants\\s*::\\s*skip\\b.*\\))\\s*\\]$"))

; unsafe: `unsafe fn`.
(function_item (function_modifiers "unsafe")) @skip.unsafe

; constructor: `fn new` in an impl or a trait (a free `fn new` is mutated).
(impl_item body: (declaration_list
  ((function_item name: (identifier) @_name) @skip.constructor (#eq? @_name "new"))))
(trait_item body: (declaration_list
  ((function_item name: (identifier) @_name) @skip.constructor (#eq? @_name "new"))))

; default_impl: `impl Default for T` (trait path ending in Default).
((impl_item trait: (type_identifier) @_trait) @skip.default_impl (#eq? @_trait "Default"))
((impl_item trait: (scoped_type_identifier name: (type_identifier) @_trait)) @skip.default_impl
  (#eq? @_trait "Default"))

; capacity_call: a call to `with_capacity`, by path or as a method.
((call_expression function: (identifier) @_fn) @skip.capacity_call (#eq? @_fn "with_capacity"))
((call_expression function: (scoped_identifier name: (identifier) @_fn)) @skip.capacity_call
  (#eq? @_fn "with_capacity"))
((call_expression function: (field_expression field: (field_identifier) @_fn)) @skip.capacity_call
  (#eq? @_fn "with_capacity"))


; Attributes on a field initializer or a match arm are its children.
((field_initializer (attribute_item) @_attr) @skip.test (#match? @_attr "^#\\[\\s*(cfg\\s*\\(\\s*test\\s*\\)|([A-Za-z_][A-Za-z0-9_]*\\s*::\\s*)*test(\\s*\\(.*\\))?)\\s*\\]$"))
((field_initializer (attribute_item) @_attr) @skip.annotated (#match? @_attr "^#\\[\\s*(mutants\\s*::\\s*skip|cfg_attr\\s*\\(.*\\bmutants\\s*::\\s*skip\\b.*\\))\\s*\\]$"))
((shorthand_field_initializer (attribute_item) @_attr) @skip.test (#match? @_attr "^#\\[\\s*(cfg\\s*\\(\\s*test\\s*\\)|([A-Za-z_][A-Za-z0-9_]*\\s*::\\s*)*test(\\s*\\(.*\\))?)\\s*\\]$"))
((shorthand_field_initializer (attribute_item) @_attr) @skip.annotated (#match? @_attr "^#\\[\\s*(mutants\\s*::\\s*skip|cfg_attr\\s*\\(.*\\bmutants\\s*::\\s*skip\\b.*\\))\\s*\\]$"))
((match_arm (attribute_item) @_attr) @skip.test (#match? @_attr "^#\\[\\s*(cfg\\s*\\(\\s*test\\s*\\)|([A-Za-z_][A-Za-z0-9_]*\\s*::\\s*)*test(\\s*\\(.*\\))?)\\s*\\]$"))
((match_arm (attribute_item) @_attr) @skip.annotated (#match? @_attr "^#\\[\\s*(mutants\\s*::\\s*skip|cfg_attr\\s*\\(.*\\bmutants\\s*::\\s*skip\\b.*\\))\\s*\\]$"))
