; Python concept captures. Capture names are the language-neutral contract:
; @concept.<concept>, @stmt.<kind>, and role names. See cherenkov-syntax.

; --- Binary: arithmetic, boolean, comparison.
; A chained comparison (a < b < c) yields one Binary with several @op
; captures, which the core reports as unrepresentable.
(binary_operator left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(boolean_operator left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(comparison_operator operators: _ @op) @concept.binary

(unary_operator operator: _ @op argument: (_) @operand) @concept.unary
(not_operator "not" @op argument: (_) @operand) @concept.unary

(augmented_assignment left: (_) @target operator: _ @op right: (_) @value) @concept.compound_assign
(assignment left: (_) @target right: (_) @value) @concept.assign

[(true) (false) (integer) (float) (string)] @concept.literal

; --- Conditions. The first alternative (elif_clause or else_clause) is the
; false target. Conditions of elif clauses themselves are not yet mapped:
; their false target is the next sibling clause, which the model cannot name.
(if_statement condition: (_) @cond consequence: (block) @then alternative: (_)? @else) @concept.condition
(while_statement condition: (_) @cond body: (block) @then alternative: (_)? @else) @concept.condition

(return_statement (_)? @value) @concept.return
(call function: (_) @callee arguments: (_) @args) @concept.call
(subscript value: (_) @base subscript: (_) @index) @concept.index

(function_definition name: (identifier) @name return_type: (type)? @returns body: (block) @body) @concept.function

; --- Statements.
(expression_statement (call)) @stmt.call
(expression_statement (assignment)) @stmt.assign
(expression_statement (augmented_assignment)) @stmt.assign
(return_statement) @stmt.return
(if_statement) @stmt.if
(while_statement) @stmt.loop
(for_statement) @stmt.loop

; --- Match cases; `case _` is the catch-all. Python has no struct literal
; with defaulted fields, so FieldInit is unmapped.
(match_statement body: (block (case_clause guard: (if_clause (_) @guard)? consequence: (block) @value) @concept.arm)) @scope
((case_clause (case_pattern) @catch_all) @concept.arm (#eq? @catch_all "_"))
