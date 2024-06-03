---
id: intro
title: Try Cala
slug: /
---

## Install Docker and Docker Compose
* choose the install method for your system https://docs.docker.com/desktop/

## Download the docker-compose.yml
```bash
wget https://raw.githubusercontent.com/GaloyMoney/cala/main/docker-compose.yml
```

## Run the Cala server with a PostgresQL instance
```bash
docker-compose up -d cala-server
```

## GraphQL demo
* open the local GraphQL playground: <br />
http://localhost:2252/graphql
* continue with the [GraphQL API demo](/docs/demo/create-journal-and-accounts)

## Cleanup
* run in the directory where the docker-compose.yml file is
```
docker-compose down
```
