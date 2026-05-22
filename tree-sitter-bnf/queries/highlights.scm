; Field labels
(fieldLabel) @label

; Non-terminal references
(nonTerminal) @variable

; Rule name (definition site — direct child of rule, i.e. the LHS)
(rule (nonTerminal) @variable.definition)

; Terminals
(literal) @string
(pattern) @string.regexp

; Kleene operators
(plus) @operator
(asterisk) @operator
(questionMark) @operator

; Comments
(comment) @comment

; Alias group
(aliasName (nonTerminal) @type)
"=>" @operator

; Precedence annotations
"%" @operator
(precKind) @keyword.operator
(integer) @number

; Directives
"%conflicts" @keyword
"%inline" @keyword
"%supertypes" @keyword
"[" @punctuation.bracket
"]" @punctuation.bracket

; Structural punctuation
"->" @operator
"|" @operator
";" @punctuation.delimiter
"(" @punctuation.bracket
")" @punctuation.bracket
"<<" @punctuation.bracket
">>" @punctuation.bracket
