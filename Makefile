# lun — build/install helpers (Phase 10).
#
# Targets:
#   make            debug build (default)
#   make release    release build, stripped, at target/release/lun
#   make test       full test suite (cargo test)
#   make install    install the release binary to $(PREFIX)/bin
#   make install-completions  install zsh completion script to ~/.zsh/completions
#   make uninstall  remove the installed binary
#   make clean      remove build artifacts

PREFIX  ?= $(HOME)/.local
BIN     := lun
ZSH_COMPLETIONS_DIR ?= $(HOME)/.zsh/completions

.PHONY: all release test install install-completions uninstall clean

all: release

release:
	cargo build --release

test:
	cargo test

install: release
	@mkdir -p "$(PREFIX)/bin"
	@cp target/release/$(BIN) "$(PREFIX)/bin/$(BIN)"
	@echo "installed $(BIN) to $(PREFIX)/bin/$(BIN)"
	@if command -v $(BIN) >/dev/null 2>&1; then \
		if [ "$$(command -v $(BIN))" != "$(PREFIX)/bin/$(BIN)" ]; then \
			echo "note: '$(BIN)' on your PATH resolves to $$(command -v $(BIN)) — make sure $(PREFIX)/bin comes first"; \
		fi; \
	else \
		echo "note: make sure $(PREFIX)/bin is on your PATH (e.g. export PATH=\"$(PREFIX)/bin:\$$PATH\" in your shell rc)"; \
	fi

install-completions:
	@mkdir -p "$(ZSH_COMPLETIONS_DIR)"
	@cp completions/_lun "$(ZSH_COMPLETIONS_DIR)/_lun"
	@echo "installed zsh completion: $(ZSH_COMPLETIONS_DIR)/_lun"
	@echo "add to ~/.zshrc:"
	@echo "  fpath=($(ZSH_COMPLETIONS_DIR) \$$fpath)"
	@echo "  autoload -Uz compinit && compinit"

uninstall:
	@rm -f "$(PREFIX)/bin/$(BIN)"
	@echo "removed $(PREFIX)/bin/$(BIN)"

clean:
	cargo clean
