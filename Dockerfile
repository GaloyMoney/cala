FROM rust:1.78 AS build
COPY . /src
WORKDIR /src
# Disable SQLx's compile-time verification since we don't have a DB during build
ENV SQLX_OFFLINE=true
RUN cargo build --locked --bin cala-server

FROM ubuntu
COPY --from=build /src/target/debug/cala-server /usr/local/bin
CMD ["cala-server"]