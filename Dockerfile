FROM rust:1.93-bookworm AS builder

WORKDIR /app
RUN cargo install dioxus-cli --version 0.7.3 --locked
COPY . .
RUN dx bundle --release --platform web && \
    rm -rf ~/.cargo/registry/src ~/.cargo/registry/cache
RUN mkdir -p /out && \
    cp "$(find target/dx -type f -path '*/web/gmail_router')" /out/app && \
    cp -r "$(find target/dx -type d -path '*/web/public')" /out/public

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /out/app /app/app
COPY --from=builder /out/public /app/public

EXPOSE 8080

CMD ["./app"]
