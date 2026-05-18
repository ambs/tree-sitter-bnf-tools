CARGO       ?= cargo
TS          ?= tree-sitter
GRAMMAR_DIR := tree-sitter-bnf

.DEFAULT_GOAL := help

.PHONY: help generate test-grammar build release test check lint fmt fmt-check clean

help: ## Show this help
	@echo "Usage: make <target>"
	@grep -E '^[a-zA-Z_-]+:.*?##' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  %-16s %s\n", $$1, $$2}'

generate: ## Regenerate parser from grammar.js
	cd $(GRAMMAR_DIR) && $(TS) generate

test-grammar: ## Run tree-sitter corpus tests
	cd $(GRAMMAR_DIR) && $(TS) test

build: ## Build both crates (debug)
	$(CARGO) build

release: ## Build both crates (release)
	$(CARGO) build --release

test: ## Run all Rust tests
	$(CARGO) test

check: ## Fast type-check without linking
	$(CARGO) check

lint: ## Run clippy
	$(CARGO) clippy -- -D warnings

fmt: ## Format Rust source
	$(CARGO) fmt

fmt-check: ## Check formatting without modifying
	$(CARGO) fmt --check

clean: ## Remove build artifacts
	$(CARGO) clean
	$(MAKE) -C $(GRAMMAR_DIR) clean
