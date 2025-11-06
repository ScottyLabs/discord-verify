#!/usr/bin/env -S just --justfile

# Show this help message
help:
    @just --list

# Build Docker images
build:
    docker compose build

# Start all services
up:
    docker compose up -d

# Stop all services
down:
    docker compose down

# Show logs for all services
logs:
    docker compose logs -f

# Show logs for app only
logs-app:
    docker compose logs -f app

# Show logs for redis only
logs-redis:
    docker compose logs -f redis

# Remove all containers and volumes
clean:
    docker compose down -v

# Remove all containers, volumes, and images
clean-all:
    docker compose down -v --rmi all

# Restart all services
restart: down up

# Rebuild and restart (for code changes)
rebuild: down build up

# Check service health
health:
    @echo "Checking app health..."
    @curl -f http://localhost:3000/health || echo "App is not healthy"
    @echo "\nChecking redis health..."
    @docker compose exec redis redis-cli ping || echo "Redis is not healthy"

# Run app locally with Docker redis
dev:
    docker compose up redis -d
    cargo run
