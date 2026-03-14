SHELL := /bin/sh
.DEFAULT_GOAL := help

VPS_DIR := deploy/vps
VPS_ENV_FILE := $(VPS_DIR)/.env
VPS_ENV_EXAMPLE := $(VPS_DIR)/.env.example
VPS_SSH_ENV_FILE := $(VPS_DIR)/.ssh.env
VPS_SSH_ENV_EXAMPLE := $(VPS_DIR)/.ssh.env.example
VPS_COMPOSE_FILE := $(VPS_DIR)/docker-compose.yml

-include $(VPS_SSH_ENV_FILE)

VPS_USER ?= root
VPS_PORT ?= 22
VPS_PATH ?= /opt/flytunnel-vps
SSH_KEY ?=

REMOTE_TARGET := $(VPS_USER)@$(VPS_HOST)
COMPOSE_LOCAL := docker compose --project-directory $(VPS_DIR) --env-file $(VPS_ENV_FILE) -f $(VPS_COMPOSE_FILE)

.PHONY: help vps-help vps-init vps-config vps-build vps-up vps-down vps-restart vps-logs vps-status vps-shell \
	vps-remote-preflight vps-remote-sync vps-remote-deploy vps-remote-logs vps-remote-status vps-remote-restart vps-remote-down

help:
	@echo "FlyTunnel VPS ops kit"
	@echo ""
	@echo "Local targets:"
	@echo "  make vps-init            Create deploy/vps/.env and deploy/vps/.ssh.env if missing"
	@echo "  make vps-config          Render docker compose config"
	@echo "  make vps-build           Build the frps image locally"
	@echo "  make vps-up              Start frps locally with docker compose"
	@echo "  make vps-down            Stop local frps stack"
	@echo "  make vps-restart         Restart local frps stack"
	@echo "  make vps-logs            Tail local frps logs"
	@echo "  make vps-status          Show local frps status"
	@echo "  make vps-shell           Open a shell inside the local frps container"
	@echo ""
	@echo "Remote targets:"
	@echo "  make vps-remote-preflight Check local ssh/rsync and remote docker availability"
	@echo "  make vps-remote-sync      Sync deploy/vps to the remote VPS path"
	@echo "  make vps-remote-deploy    Sync and deploy frps on the VPS"
	@echo "  make vps-remote-logs      Tail remote frps logs"
	@echo "  make vps-remote-status    Show remote frps status"
	@echo "  make vps-remote-restart   Restart remote frps stack"
	@echo "  make vps-remote-down      Stop remote frps stack"

vps-help: help

vps-init:
	@if [ ! -f "$(VPS_ENV_FILE)" ]; then cp "$(VPS_ENV_EXAMPLE)" "$(VPS_ENV_FILE)"; echo "Created $(VPS_ENV_FILE)"; else echo "Keeping existing $(VPS_ENV_FILE)"; fi
	@if [ ! -f "$(VPS_SSH_ENV_FILE)" ]; then cp "$(VPS_SSH_ENV_EXAMPLE)" "$(VPS_SSH_ENV_FILE)"; echo "Created $(VPS_SSH_ENV_FILE)"; else echo "Keeping existing $(VPS_SSH_ENV_FILE)"; fi

vps-config: vps-init
	@$(COMPOSE_LOCAL) config

vps-build: vps-init
	@$(COMPOSE_LOCAL) build

vps-up: vps-init
	@$(COMPOSE_LOCAL) up -d --build

vps-down: vps-init
	@$(COMPOSE_LOCAL) down

vps-restart: vps-init
	@$(COMPOSE_LOCAL) restart

vps-logs: vps-init
	@$(COMPOSE_LOCAL) logs -f --tail=200 frps

vps-status: vps-init
	@$(COMPOSE_LOCAL) ps

vps-shell: vps-init
	@$(COMPOSE_LOCAL) exec frps /bin/sh

vps-remote-preflight: vps-init
	@command -v ssh >/dev/null 2>&1 || { echo "Missing local dependency: ssh"; exit 1; }
	@command -v rsync >/dev/null 2>&1 || { echo "Missing local dependency: rsync"; exit 1; }
	@[ -n "$(strip $(VPS_HOST))" ] || { echo "Set VPS_HOST in $(VPS_SSH_ENV_FILE) before running remote targets."; exit 1; }
	@ssh_cmd='ssh -p $(VPS_PORT)'; \
	if [ -n "$(strip $(SSH_KEY))" ]; then ssh_cmd="$$ssh_cmd -i $(SSH_KEY)"; fi; \
	echo "Checking remote host $(REMOTE_TARGET)"; \
	$$ssh_cmd "$(REMOTE_TARGET)" "set -eu; mkdir -p '$(VPS_PATH)'; docker --version >/dev/null; docker compose version >/dev/null; echo 'Remote Docker OK at $(VPS_PATH)'"

vps-remote-sync: vps-init vps-remote-preflight
	@ssh_cmd='ssh -p $(VPS_PORT)'; \
	if [ -n "$(strip $(SSH_KEY))" ]; then ssh_cmd="$$ssh_cmd -i $(SSH_KEY)"; fi; \
	$$ssh_cmd "$(REMOTE_TARGET)" "mkdir -p '$(VPS_PATH)'"; \
	rsync -az --delete --exclude '.ssh.env' --exclude '.ssh.env.example' -e "$$ssh_cmd" "$(VPS_DIR)/" "$(REMOTE_TARGET):$(VPS_PATH)/"; \
	echo "Synced $(VPS_DIR)/ -> $(REMOTE_TARGET):$(VPS_PATH)/"

vps-remote-deploy: vps-config vps-remote-sync
	@ssh_cmd='ssh -p $(VPS_PORT)'; \
	if [ -n "$(strip $(SSH_KEY))" ]; then ssh_cmd="$$ssh_cmd -i $(SSH_KEY)"; fi; \
	$$ssh_cmd "$(REMOTE_TARGET)" "set -eu; cd '$(VPS_PATH)'; docker compose config >/dev/null; docker compose up -d --build; docker compose ps"

vps-remote-logs: vps-remote-preflight
	@ssh_cmd='ssh -p $(VPS_PORT)'; \
	if [ -n "$(strip $(SSH_KEY))" ]; then ssh_cmd="$$ssh_cmd -i $(SSH_KEY)"; fi; \
	$$ssh_cmd "$(REMOTE_TARGET)" "set -eu; cd '$(VPS_PATH)'; docker compose logs -f --tail=200 frps"

vps-remote-status: vps-remote-preflight
	@ssh_cmd='ssh -p $(VPS_PORT)'; \
	if [ -n "$(strip $(SSH_KEY))" ]; then ssh_cmd="$$ssh_cmd -i $(SSH_KEY)"; fi; \
	$$ssh_cmd "$(REMOTE_TARGET)" "set -eu; cd '$(VPS_PATH)'; docker compose ps"

vps-remote-restart: vps-remote-preflight
	@ssh_cmd='ssh -p $(VPS_PORT)'; \
	if [ -n "$(strip $(SSH_KEY))" ]; then ssh_cmd="$$ssh_cmd -i $(SSH_KEY)"; fi; \
	$$ssh_cmd "$(REMOTE_TARGET)" "set -eu; cd '$(VPS_PATH)'; docker compose restart"

vps-remote-down: vps-remote-preflight
	@ssh_cmd='ssh -p $(VPS_PORT)'; \
	if [ -n "$(strip $(SSH_KEY))" ]; then ssh_cmd="$$ssh_cmd -i $(SSH_KEY)"; fi; \
	$$ssh_cmd "$(REMOTE_TARGET)" "set -eu; cd '$(VPS_PATH)'; docker compose down"
