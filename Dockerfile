# Build Stage
FROM rust:1.75 as builder
WORKDIR /usr/src/sw4-account-creator
COPY . .
RUN cargo build --release

# Runtime Stage
FROM ubuntu:22.04
# Set the working directory be able to find templates and assets
WORKDIR /usr/local/bin
# Install necessary runtime libraries
RUN apt-get update && apt-get install -y \
    openssl libcurl4 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# Copy the binary from the builder stage
COPY --from=builder /usr/src/sw4-account-creator/target/release/sw4-account-creator /usr/local/bin/sw4-account-creator
# Copying templates and assets
COPY --from=builder /usr/src/sw4-account-creator/templates /usr/local/bin/templates
COPY --from=builder /usr/src/sw4-account-creator/assets /usr/local/bin/assets
ENTRYPOINT ["sw4-account-creator"]
