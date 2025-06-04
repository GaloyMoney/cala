FROM clux/muslrust:stable AS build
  COPY . /src
  WORKDIR /src
  RUN SQLX_OFFLINE=true cargo build --locked --bin cala-server

FROM ubuntu
  COPY --from=build /src/target/x86_64-unknown-linux-musl/debug/cala-server /usr/local/bin
  RUN mkdir /cala-server
  RUN chown -R 1000 /cala-server && chmod -R u+w /cala-server
  USER 1000
  WORKDIR /cala-server
  CMD ["cala-server"]