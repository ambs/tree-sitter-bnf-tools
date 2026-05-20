; Inject the regex language into pattern node content,
; excluding the surrounding / delimiters.
((pattern) @injection.content
 (#set! injection.language "regex")
 (#offset! @injection.content 0 1 0 -1))
