BIN     := target/debug/plexus-substrate
LOG     := /tmp/substrate.log
PIDFILE := /tmp/substrate.pid

.PHONY: build restart start stop log

build:
	cargo build --package plexus-substrate --features mcp-gateway

start: build
	@if [ -f $(PIDFILE) ] && kill -0 $$(cat $(PIDFILE)) 2>/dev/null; then \
		echo "substrate already running (pid $$(cat $(PIDFILE)))"; \
	else \
		nohup $(BIN) > $(LOG) 2>&1 & echo $$! > $(PIDFILE); \
		echo "substrate started (pid $$(cat $(PIDFILE)))"; \
	fi

stop:
	@if [ -f $(PIDFILE) ]; then \
		kill $$(cat $(PIDFILE)) 2>/dev/null || true; \
		rm -f $(PIDFILE); \
	fi
	@echo "substrate stopped"

restart: stop build
	@sleep 1
	@nohup $(BIN) > $(LOG) 2>&1 & echo $$! > $(PIDFILE)
	@echo "substrate restarted (pid $$(cat $(PIDFILE)))"

log:
	@tail -f $(LOG)
