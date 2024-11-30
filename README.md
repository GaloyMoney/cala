# Cala

Cala is a robust ledger system developed by Galoy, designed to handle complex financial transactions and accounting operations. It provides a flexible and scalable solution for managing financial records with strong consistency guarantees.

## Components

- `cala-ledger`: Core ledger implementation
- `cala-server`: Server implementation handling API requests
- `cala-nodejs`: Node.js bindings for integration with JavaScript/TypeScript applications
- `cala-cel-interpreter` & `cala-cel-parser`: Common Expression Language (CEL) support
- `cala-tracing`: Tracing and monitoring functionality

## Prerequisites

The following dependencies are required:
- Rust (see `rust-toolchain.toml` for version)
- Node.js (for Node.js bindings)
- Docker (for containerized deployment)
- Make

All dependencies can be automatically installed using Nix:
```bash
nix develop
```

This will set up a development environment with all required tools and dependencies.

## Development Setup

1. Clone the repository:
```bash
git clone https://github.com/GaloyMoney/cala.git
cd cala
```

2. Install dependencies:
```bash
make reset-deps
```

## Testing

Run unit tests with:
```bash
make reset-deps next-watch
```

Run end-to-end tests with:
```bash
make e2e
```

## Running the Server

To run the server:
```bash
make run-server
```

## Docker Support

Build the Docker image:
```bash
docker build -t cala .
```

For production builds:
```bash
docker build -f Dockerfile.release -t cala:release .
```
