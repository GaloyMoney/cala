---
id: intro
title: Intro
slug: /
---

# Cala documentation

An embeddable double sided accounting ledger built on PG/SQLx

## Build

### Dependencies
#### Nix package manager
* recommended install method using https://github.com/DeterminateSystems/nix-installer
  ```
  curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install
  ```

#### direnv >= 2.30.0
* recommended install method from https://direnv.net/docs/installation.html:
  ```
  curl -sfL https://direnv.net/install.sh | bash
  echo "eval \"\$(direnv hook bash)\"" >> ~/.bashrc
  source ~/.bashrc
  ```

#### Docker
* choose the install method for your system https://docs.docker.com/desktop/

## Build from source
### Download the source code
```
git clone https://github.com/GaloyMoney/cala
```
### Build and run the server
```
cd cala
direnv allow
make reset-deps # clean-deps start-deps setup-db
make run-server
```

## Use the precompiled binary
### Download and install the binary
```
wget https://github.com/GaloyMoney/cala/releases/download/0.1.3/cala-server-x86_64-unknown-linux-musl-0.1.3.tar.gz
tar -xvf cala-server-x86_64-unknown-linux-musl-0.1.3.tar.gz
cd cala-server-x86_64-unknown-linux-musl-0.1.3
sudo cp ./cala-server /usr/local/bin/
```

### Run the Cala server
```
git clone https://github.com/GaloyMoney/cala
cd cala
make start-deps
cala-server --config ./bats/cala.yml
```

## Live demo
* continue on the [Demo tab](/demo)

## Cleanup
```
make clean-deps
```
