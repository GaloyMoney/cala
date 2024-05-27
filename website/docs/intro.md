---
id: intro
title: Intro
slug: /
---

# Cala documentation

An embeddable double sided accounting ledger built on PG/SQLx

## Use the precompiled binary
## need to have Docker installed
* choose the install method for your system https://docs.docker.com/desktop/

### Download and install cala-server binary
```
wget https://github.com/GaloyMoney/cala/releases/download/0.1.3/cala-server-x86_64-unknown-linux-musl-0.1.3.tar.gz
tar -xvf cala-server-x86_64-unknown-linux-musl-0.1.3.tar.gz
cd cala-server-x86_64-unknown-linux-musl-0.1.3
sudo cp ./cala-server /usr/local/bin/
```

### Run the Cala server
#### Start the dependencies in Docker
```
git clone https://github.com/GaloyMoney/cala
cd cala
make start-deps
```

#### Start the server
```
cala-server --config ./bats/cala.yml
```

## GraphQL demo
* continue [Demo start page](/demo/start)

## Cleanup
```
make clean-deps
```
