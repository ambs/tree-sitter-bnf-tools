CARGO       ?= cargo
TS          ?= tree-sitter
GRAMMAR_DIR := tree-sitter-bnf
PARSER_C    := $(GRAMMAR_DIR)/src/parser.c

.DEFAULT_GOAL := help

.PHONY: help generate test-grammar build release test check lint fmt fmt-check clean

help: ## Show this help
	@echo "Usage: make <target>"
	@grep -E '^[a-zA-Z_-]+:.*?##' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  %-16s %s\n", $$1, $$2}'

$(PARSER_C): $(GRAMMAR_DIR)/grammar.js
	cd $(GRAMMAR_DIR) && $(TS) generate

generate: $(PARSER_C) ## Regenerate parser from grammar.js (runs only if grammar.js changed)

test-grammar: $(PARSER_C) ## Run tree-sitter corpus tests
	cd $(GRAMMAR_DIR) && $(TS) test

build: $(PARSER_C) ## Build both crates (debug)
	$(CARGO) build

release: $(PARSER_C) ## Build both crates (release)
	$(CARGO) build --release

test: $(PARSER_C) ## Run all Rust tests
	$(CARGO) test

check: $(PARSER_C) ## Fast type-check without linking
	$(CARGO) check

lint: $(PARSER_C) ## Run clippy
	$(CARGO) clippy -- -D warnings

fmt: ## Format Rust source
	$(CARGO) fmt

fmt-check: ## Check formatting without modifying
	$(CARGO) fmt --check

clean: ## Remove build artifacts
	$(CARGO) clean
	rm -rf $(GRAMMAR_DIR)/src
