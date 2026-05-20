; The whole file is one scope
(grammar) @local.scope

; Rule LHS — the non-terminal being defined
(rule
  (nonTerminal) @definition.function)

; Non-terminal used in a rule body — a reference to a defined rule
(symbol
  (nonTerminal) @reference.function)
