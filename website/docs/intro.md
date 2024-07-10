---
id: intro
title: Try Cala
slug: /docs
---

## Install Docker and Docker Compose
* choose the install method for your system https://docs.docker.com/desktop/

## Download the docker-compose.yml
```bash
wget https://raw.githubusercontent.com/GaloyMoney/cala/main/docker-compose.yml
```

## Run the Cala Server with a PostgresQL Instance
```bash
docker-compose up -d cala-server
```

## GraphQL Demo
* open the local GraphQL playground: <br />
http://localhost:2252/graphql
* continue with the [GraphQL API Demo](/docs/create-journal-and-accounts)

## Cleanup
* run in the directory where the docker-compose.yml file is
```
docker-compose down
```
