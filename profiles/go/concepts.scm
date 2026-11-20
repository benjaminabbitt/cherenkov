; Go concept captures. See profiles/python/concepts.scm for the contract.

(binary_expression left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(unary_expression operator: _ @op operand: (_) @operand) @concept.unary
(assignment_statement
  left: (_) @target
  operator: ["+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>=" "&^="] @op
  right: (_) @value) @concept.compound_assign
(assignment_statement left: (_) @target operator: "=" right: (_) @value) @concept.assign
(short_var_declaration left: (_) @target right: (_) @value) @concept.assign

[(true) (false) (int_literal) (float_literal) (interpreted_string_literal) (raw_string_literal)] @concept.literal

(if_statement condition: (_) @cond consequence: (block) @then alternative: (_)? @else) @concept.condition
; `for cond {}` has no condition field; `for init; cond; post {}` does.
(for_statement
  [(binary_expression) (identifier) (unary_expression) (call_expression)
   (parenthesized_expression) (selector_expression) (true) (false)] @cond
  body: (block) @then) @concept.condition
(for_statement (for_clause condition: (_) @cond) body: (block) @then) @concept.condition

; Single-valued returns only; `return a, b` has no single value.
(return_statement (expression_list . (_) @value .)) @concept.return
(call_expression function: (_) @callee arguments: (_) @args) @concept.call
(index_expression operand: (_) @base index: (_) @index) @concept.index

(function_declaration name: (identifier) @name result: (_)? @returns body: (block) @body) @concept.function
(method_declaration name: (field_identifier) @name result: (_)? @returns body: (block) @body) @concept.function

(expression_statement (call_expression)) @stmt.call
(assignment_statement) @stmt.assign
(short_var_declaration) @stmt.assign
(inc_statement) @stmt.assign
(dec_statement) @stmt.assign
(return_statement) @stmt.return
(if_statement) @stmt.if
(for_statement) @stmt.loop

; --- Switch cases; `default` is the catch-all. Go has no guards.
(expression_switch_statement [(expression_case) (default_case)] @concept.arm) @scope
(type_switch_statement [(type_case) (default_case)] @concept.arm) @scope
(default_case) @concept.arm @catch_all
; A case's body, which a deleted case is redirected from (or to, for default).
(expression_case (statement_list) @value) @concept.arm
(type_case (statement_list) @value) @concept.arm
(default_case (statement_list) @value) @concept.arm

; --- Struct literal fields: keyed elements of a literal of a named type;
; omitted fields are zero. Map and slice literals have their own type nodes.
(composite_literal
  type: [(type_identifier) (qualified_type) (generic_type)]
  body: (literal_value (keyed_element) @concept.field_init)) @scope
