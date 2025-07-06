# Base image with compiler
FROM rust:1.88.0-slim AS build

# Directory where application is built
WORKDIR /app

# Copy project manifest into container
COPY Cargo.lock Cargo.toml .

# Build dependencies in separate step, to preserve them across application code changes
RUN mkdir src
RUN echo 'fn main() {}' > src/main.rs
RUN cargo build --release
RUN rm -r ./target/release/.fingerprint/signal-http-*

# Copy application code into container
COPY src src

# Build binary
RUN cargo build --release

# Somewhat minimal base image of deployed container
FROM debian:12.10-slim

# Import build artifact from build step
COPY --from=build /app/target/release/signal-http .

# Define main process of container
ENTRYPOINT [ "./signal-http" ]
