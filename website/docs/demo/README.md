---
id: start
title: Demo start
slug: /demo/start
---

Make sure cala-server and your nginx proxy is running and try the GraphQL functions in the live editor:

* [Journalcreate](./journalcreate.mdx)


## Install an nginx proxy locally

To be able to call the cala server from the browser will need to set up a simple nginx proxy.

1. Create an `nginx.conf` file with the contents:

```
server {
    listen 80;

    location /graphql {
        proxy_pass http://172.17.0.1:2252;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;

        # Add CORS headers
        add_header 'Access-Control-Allow-Origin' '*';
        add_header 'Access-Control-Allow-Methods' 'GET, POST, OPTIONS';
        add_header 'Access-Control-Allow-Headers' 'Origin, Content-Type, Accept, Authorization';

        # Handle preflight requests
        if ($request_method = 'OPTIONS') {
            add_header 'Access-Control-Allow-Origin' '*';
            add_header 'Access-Control-Allow-Methods' 'GET, POST, OPTIONS';
            add_header 'Access-Control-Allow-Headers' 'Origin, Content-Type, Accept, Authorization';
            add_header 'Access-Control-Max-Age' 1728000;
            add_header 'Content-Type' 'text/plain charset=UTF-8';
            add_header 'Content-Length' 0;
            return 204;
        }
    }
}
```

2. Create a `Dockerfile` with the following content:
```
FROM nginx:alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
```

3. Build and Run the Docker Container:
```
docker build -t my-nginx-proxy .
docker run -d -p 8080:80 --name nginx-proxy my-nginx-proxy
```
