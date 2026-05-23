; Rule body: indent after `->`, dedent at `;`.
(rule "->" @indent)
(rule ";" @dedent)

; `|` opens a branch at the same level as the preceding alternative.
(ruleBody "|" @branch)
(ruleBodyInner "|" @branch)

; Grouped sub-expressions: parenthesised groups and token expressions.
["(" "<<" "<<!"] @indent
[")" ">>"] @dedent
