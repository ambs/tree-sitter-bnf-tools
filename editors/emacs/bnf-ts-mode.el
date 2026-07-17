;;; bnf-ts-mode.el --- Tree-sitter major mode for the BNF dialect -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Alberto Simões

;; Author: Alberto Simões
;; URL: https://github.com/ambs/tree-sitter-bnf-tools
;; Keywords: languages
;; Package-Requires: ((emacs "29.1"))

;; This file is not part of GNU Emacs.

;; MIT License. See the LICENSE file at the root of the
;; tree-sitter-bnf-tools repository for the full text.

;;; Commentary:

;; A `treesit'-based major mode for the BNF dialect used by
;; tree-sitter-bnf-tools (https://github.com/ambs/tree-sitter-bnf-tools).
;;
;; Setup, once this file is on your `load-path':
;;
;;   (require 'bnf-ts-mode)
;;
;; Before first use, build and install the grammar. `tree-sitter-bnf/src/'
;; (parser.c and friends) is gitignored — it does not exist in a fresh
;; clone, so `tree-sitter generate' must always be run, not just after a
;; grammar.js change — which also means `treesit-install-language-grammar'
;; cannot clone-and-compile this repo automatically. Compile by hand:
;;
;;   git clone https://github.com/ambs/tree-sitter-bnf-tools
;;   cd tree-sitter-bnf-tools/tree-sitter-bnf
;;   tree-sitter generate
;;   mkdir -p ~/.emacs.d/tree-sitter
;;   gcc -shared -fPIC -o ~/.emacs.d/tree-sitter/libtree-sitter-bnf.so \
;;       -I./src src/parser.c
;;
;; See docs/editors.md in the tree-sitter-bnf-tools repository for the full
;; walkthrough, including macOS (.dylib) paths and verification steps.
;;
;; Features:
;; - Syntax highlighting (mirrors tree-sitter-bnf/queries/highlights.scm)
;; - Structural navigation: `C-M-a' / `C-M-e' jump between rule definitions
;; - Imenu / `consult-imenu' integration: rule names are indexable symbols
;; - Indentation: `|' and `;' align under the `>' of the enclosing `->'

;;; Code:

(require 'treesit)

(defun bnf-ts--find-arrow (rule-node)
  "Find the \"->\" child node inside RULE-NODE."
  (let ((i 0)
        (count (treesit-node-child-count rule-node))
        found)
    (while (and (< i count) (not found))
      (let ((child (treesit-node-child rule-node i)))
        (when (string= (treesit-node-type child) "->")
          (setq found child)))
      (setq i (1+ i)))
    found))

(defun bnf-ts-indent-line ()
  "Indent the current line, aligning \"|\" and \";\" under the \">\" of \"->\"."
  (interactive)
  (save-excursion
    (back-to-indentation)
    (let* ((node (treesit-node-at (point)))
           (type (treesit-node-type node))
           (parent (treesit-node-parent node))
           ;; "|" lives inside ruleBody, so go up two levels to reach rule;
           ;; ";" is a direct child of rule, so go up one level.
           (rule-node (cond
                       ((string= type "|") (treesit-node-parent parent))
                       ((string= type ";") parent)
                       (t nil))))
      (when rule-node
        (let ((arrow (bnf-ts--find-arrow rule-node)))
          (when arrow
            (let ((col (save-excursion
                         (goto-char (treesit-node-end arrow))
                         (1- (current-column)))))
              (indent-line-to col))))))))

;;;###autoload
(define-derived-mode bnf-ts-mode prog-mode "BNF"
  "Major mode for BNF files, powered by tree-sitter."
  (when (treesit-available-p)
    (treesit-parser-create 'bnf)

    ;; Syntax highlighting (mirrors tree-sitter-bnf/queries/highlights.scm)
    (setq-local treesit-font-lock-settings
                (treesit-font-lock-rules
                 :language 'bnf
                 :feature 'comment
                 '((comment) @font-lock-comment-face)

                 :language 'bnf
                 :feature 'keyword
                 '(["%axiom" "%conflicts" "%include" "%inline"
                    "%supertypes" "%extras"] @font-lock-keyword-face
                   (precKind) @font-lock-keyword-face)

                 :language 'bnf
                 :feature 'definition
                 '((rule (nonTerminal) @font-lock-function-name-face))

                 :language 'bnf
                 :feature 'variable
                 '((nonTerminal) @font-lock-variable-name-face
                   (aliasName (nonTerminal) @font-lock-type-face))

                 :language 'bnf
                 :feature 'label
                 '((fieldLabel) @font-lock-property-name-face)

                 :language 'bnf
                 :feature 'string
                 '((literal) @font-lock-string-face
                   (pattern) @font-lock-regexp-grouping-construct)

                 :language 'bnf
                 :feature 'number
                 '((integer) @font-lock-number-face)

                 :language 'bnf
                 :feature 'operator
                 '([(plus) (asterisk) (questionMark)
                    "=>" "%" "->" "|"] @font-lock-operator-face)

                 :language 'bnf
                 :feature 'bracket
                 '(["[" "]" "(" ")" "<<" "<<!" ">>"] @font-lock-bracket-face)

                 :language 'bnf
                 :feature 'delimiter
                 '(";" @font-lock-delimiter-face)))

    ;; Enable features in order: earlier = higher priority
    (setq-local treesit-font-lock-feature-list
                '((comment definition)
                  (keyword string)
                  (variable label number)
                  (operator bracket delimiter)))

    ;; Structural navigation: C-M-a / C-M-e jump between rule definitions.
    ;; The rule's non-terminal is exposed via the "name" field (see
    ;; `rule' in tree-sitter-bnf/grammar.js), not the "nonTerminal" node
    ;; type — field name and node type happen to differ here.
    (setq-local treesit-defun-type-regexp "rule")
    (setq-local treesit-defun-name-function
                (lambda (node)
                  (when (string= (treesit-node-type node) "rule")
                    (treesit-node-text
                     (treesit-node-child-by-field-name node "name") t))))

    ;; Imenu / consult-imenu integration: rule names as jumpable entries.
    (setq-local treesit-simple-imenu-settings
                '(("Rule" "\\`rule\\'" nil
                   (lambda (node)
                     (treesit-node-text
                      (treesit-node-child-by-field-name node "name") t)))))

    ;; Indentation: | and ; align under the > of the enclosing ->.
    (setq-local indent-line-function #'bnf-ts-indent-line)

    (treesit-major-mode-setup)))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.bnf\\'" . bnf-ts-mode))

;; Optional: buffer-text completion restricted to identifier characters,
;; via `dabbrev' (built in) and `cape-dabbrev' (from the `cape' package,
;; https://github.com/minad/cape). Delete this block if you don't use cape.
(add-hook 'bnf-ts-mode-hook
          (lambda ()
            (require 'dabbrev)
            (setq-local dabbrev-abbrev-char-regexp "[A-Za-z_][A-Za-z_0-9]*")
            (when (fboundp 'cape-dabbrev)
              (add-to-list 'completion-at-point-functions #'cape-dabbrev t))))

(provide 'bnf-ts-mode)

;;; bnf-ts-mode.el ends here
