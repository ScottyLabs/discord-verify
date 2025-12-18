#!/usr/bin/env -S just --justfile

# Show this help message
help:
    @just --list

# Show logs for all services
logs:
    docker compose logs -f

# Remove all containers and volumes
clean:
    docker compose down -v

# Remove all containers, volumes, and images
clean-all:
    docker compose down -v --rmi all

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
