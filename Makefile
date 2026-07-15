CARGO       ?= cargo
TS          ?= tree-sitter
TS_MIN      := 0.24.4
GRAMMAR_DIR := tree-sitter-bnf
PARSER_C    := $(GRAMMAR_DIR)/src/parser.c
BNF_TOOL    := $(CARGO) run --quiet -p ts-bnf-tool --
GRAMMAR_BNF := grammar/bnf.bnf
RAILROAD    := grammar/railroad.svg
GRAPH_PDF   := grammar/graph.pdf

.DEFAULT_GOAL := help

.PHONY: help generate test-grammar ts-version-check build release test check typecheck lint fmt fmt-check clean publish install grammar grammar-check audit

help: ## Show this help
	@echo "Usage: make <target>"
	@grep -E '^[a-zA-Z_-]+:.*?##' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  %-16s %s\n", $$1, $$2}'

$(PARSER_C): $(GRAMMAR_DIR)/grammar.js
	cd $(GRAMMAR_DIR) && $(TS) generate

generate: $(PARSER_C) ## Regenerate parser from grammar.js (runs only if grammar.js changed)

$(RAILROAD): $(GRAMMAR_BNF) $(PARSER_C)
	$(BNF_TOOL) railroad $(GRAMMAR_BNF) -o $(RAILROAD)

$(GRAPH_PDF): $(GRAMMAR_BNF) $(PARSER_C)
	$(BNF_TOOL) graph --format pdf $(GRAMMAR_BNF) -o $(GRAPH_PDF)

grammar: $(RAILROAD) $(GRAPH_PDF) ## Regenerate grammar/railroad.svg and grammar/graph.pdf from grammar/bnf.bnf

grammar-check: grammar ## Fail if grammar/railroad.svg or grammar/graph.pdf are out of date
	@git diff --exit-code $(RAILROAD) $(GRAPH_PDF) || \
		(echo "grammar-check: generated files are stale — commit $(RAILROAD) and $(GRAPH_PDF)" >&2; exit 1)

ts-version-check: ## Check that tree-sitter-cli >= TS_MIN is installed
	@TS_VER=$$($(TS) --version 2>/dev/null | sed 's/tree-sitter //'); \
	if [ -z "$$TS_VER" ]; then \
		echo "Error: tree-sitter not found. Install with: npm install -g tree-sitter-cli@$(TS_MIN)" >&2; \
		exit 1; \
	fi; \
	if [ "$$(printf '%s\n' "$(TS_MIN)" "$$TS_VER" | sort -V | head -1)" != "$(TS_MIN)" ]; then \
		echo "Error: tree-sitter >= $(TS_MIN) required (found $$TS_VER). Upgrade with: npm install -g tree-sitter-cli" >&2; \
		exit 1; \
	fi

test-grammar: ts-version-check $(PARSER_C) ## Run tree-sitter corpus tests
	cd $(GRAMMAR_DIR) && $(TS) test

build: $(PARSER_C) ## Build both crates (debug)
	$(CARGO) build

release: $(PARSER_C) ## Build both crates (release)
	$(CARGO) build --release

test: $(PARSER_C) ## Run all Rust tests
	$(CARGO) test

typecheck: $(PARSER_C) ## Fast type-check without linking
	$(CARGO) check

check: fmt-check lint typecheck test test-grammar grammar-check audit ## Run all checks (fmt, lint, typecheck, tests, corpus, audit)

lint: $(PARSER_C) ## Run clippy
	$(CARGO) clippy -- -D warnings

audit: ## Check dependencies against the RustSec advisory database
	@if ! $(CARGO) audit --version >/dev/null 2>&1; then \
		echo "Error: cargo-audit not found. Install with: cargo install cargo-audit" >&2; \
		exit 1; \
	fi
	$(CARGO) audit

fmt: ## Format Rust source
	$(CARGO) fmt

fmt-check: ## Check formatting without modifying
	$(CARGO) fmt --check

install: $(PARSER_C) ## Install ts-bnf-tool locally (cargo install --path)
	$(CARGO) install --path tools

publish: ## Publish crates to crates.io (tree-sitter-bnf first, then ts-bnf-tool)
	$(CARGO) publish -p tree-sitter-bnf --allow-dirty
	@echo "Waiting for crates.io index to update..."
	sleep 30
	$(CARGO) publish -p ts-bnf-tool --allow-dirty

clean: ## Remove build artifacts
	$(CARGO) clean
	rm -rf $(GRAMMAR_DIR)/src
