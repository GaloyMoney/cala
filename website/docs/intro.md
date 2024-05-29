---
id: intro
title: Try Cala
slug: /
---

# Run locally
## Install Docker and Docker Compose
* choose the install method for your system https://docs.docker.com/desktop/

## Download the docker-compose file to run Cala and its dependencies
```
wget https://raw.githubusercontent.com/GaloyMoney/cala/docs--website/docker-compose.yml
```

## Create a config file
```
touch cala.yml
```

## Run the Cala server
```
docker-compose up cala-server
```

## GraphQL demo
* open the local GraphQL playground: <br />
http://localhost:2252/graphql
* continue with the [GraphQL API demo](/docs/demo/create-journal-and-accounts)

## Cleanup
```
make clean-deps
```
