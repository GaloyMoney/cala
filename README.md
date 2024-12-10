# Cala

Cala is a robust ledger system developed by Galoy, designed to handle complex financial transactions and accounting operations. It provides a flexible and scalable solution for managing financial records with strong consistency guarantees.

## Features

### Core Capabilities

- **Double-Entry Accounting**: Built-in support for double-entry bookkeeping principles ensuring accurate financial records
- **SQL-Compatible**: Engineered to work with SQL databases (PostgreSQL) for robust data persistence and querying
- **Strong Consistency**: Ensures accuracy and reliability of financial records
- **Real-time Processing**: Efficient transaction processing suitable for production financial systems

### API & Integration

- **GraphQL API**: Modern API interface with built-in playground for easy integration and testing
- **Extensible Architecture**: Modular design with support for custom extensions via the Node.js bindings
- **Transaction Templates**: Customizable transaction templates for common financial operations
- **Multi-Currency Support**: Handle transactions across different currencies

## Developing

### Dependencies

#### Nix package manager

- Recommended install method using https://github.com/DeterminateSystems/nix-installer
  ```
  curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install
  ```

#### direnv >= 2.30.0

- Recommended install method from https://direnv.net/docs/installation.html:
  ```
  curl -sfL https://direnv.net/install.sh | bash
  echo "eval \"\$(direnv hook bash)\"" >> ~/.bashrc
  source ~/.bashrc
  ```

#### Docker

- Choose the install method for your system https://docs.docker.com/desktop/

### Testing

Run unit tests with:

```bash
make reset-deps next-watch
```

Run end-to-end tests with:

```bash
make e2e
```

### Running the Server

To run the server:

```bash
make run-server
```
