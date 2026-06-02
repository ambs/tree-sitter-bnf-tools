CARGO       ?= cargo
TS          ?= tree-sitter
TS_MIN      := 0.24.4
GRAMMAR_DIR := tree-sitter-bnf
PARSER_C    := $(GRAMMAR_DIR)/src/parser.c

.DEFAULT_GOAL := help

.PHONY: help generate test-grammar ts-version-check build release test check typecheck lint fmt fmt-check clean publish

help: ## Show this help
	@echo "Usage: make <target>"
	@grep -E '^[a-zA-Z_-]+:.*?##' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  %-16s %s\n", $$1, $$2}'

$(PARSER_C): $(GRAMMAR_DIR)/grammar.js
	cd $(GRAMMAR_DIR) && $(TS) generate

generate: $(PARSER_C) ## Regenerate parser from grammar.js (runs only if grammar.js changed)

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

check: fmt-check lint typecheck test test-grammar ## Run all checks (fmt, lint, typecheck, tests, corpus)

lint: $(PARSER_C) ## Run clippy
	$(CARGO) clippy -- -D warnings

fmt: ## Format Rust source
	$(CARGO) fmt

fmt-check: ## Check formatting without modifying
	$(CARGO) fmt --check

publish: ## Publish crates to crates.io (tree-sitter-bnf first, then ts-bnf-tool)
	$(CARGO) publish -p tree-sitter-bnf --allow-dirty
	@echo "Waiting for crates.io index to update..."
	sleep 30
	$(CARGO) publish -p ts-bnf-tool --allow-dirty

clean: ## Remove build artifacts
	$(CARGO) clean
	rm -rf $(GRAMMAR_DIR)/src
