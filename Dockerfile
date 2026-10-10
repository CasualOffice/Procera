# Procera server image. Build with `--build-arg FEATURES=e2e` ONLY for the E2E stack (docs/23 §4).
FROM rust:1.98-bookworm AS build
ARG FEATURES=""
WORKDIR /src
COPY . .
RUN if [ -n "$FEATURES" ]; then cargo build --release -p procera --features "$FEATURES"; \
    else cargo build --release -p procera; fi

# Non-root, minimal runtime (docs/06 deployment hardening).
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/procera /usr/local/bin/procera
USER nonroot:nonroot
EXPOSE 8080
ENV PROCERA_BIND=0.0.0.0:8080
ENTRYPOINT ["/usr/local/bin/procera"]
CMD ["serve", "--all"]
