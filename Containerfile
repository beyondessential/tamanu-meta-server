FROM --platform=$BUILDPLATFORM rust AS builder
SHELL ["/bin/bash", "-euxo", "pipefail", "-c"]

ARG PROFILE=release
ARG TARGETPLATFORM

RUN mkdir -p /app/{.cargo,src} /built && useradd --system --user-group --uid 1000 tamanu
WORKDIR /app

RUN if [ "$TARGETPLATFORM" == "linux/amd64" ]; then \
        echo "x86_64-unknown-linux-gnu" >/.target; \
        apt-get -y update; \
        apt-get -y install libc-dev; \
    elif [ "$TARGETPLATFORM" == "linux/arm64" ]; then \
        echo "aarch64-unknown-linux-gnu" >/.target; \
        dpkg --add-architecture arm64; \
        apt-get -y update; \
        apt-get -y install --no-install-recommends \
            libc-dev:arm64 \
            {binutils,gcc,g++,gfortran}-aarch64-linux-gnu; \
        echo -e '[target.aarch64-unknown-linux-gnu]\nlinker = "aarch64-linux-gnu-gcc"' >> .cargo/config.toml; \
    else echo "Unknown architecture $TARGETPLATFORM"; exit 1; \
    fi

RUN rustup target add "$(cat /.target)"
ENV RUSTFLAGS="-C target-feature=+crt-static"

# Download and build dependencies (for cache)
RUN echo "fn main() {}" > src/main.rs
COPY Cargo.lock Cargo.toml ./
RUN cargo build --locked --target $(cat /.target) --profile $PROFILE
RUN rm target/$(cat /.target)/$PROFILE/{tamanu-meta,deps/tamanu_meta*}

# Build the actual project
COPY migrations ./migrations
COPY src ./src
RUN cargo build --locked --target $(cat /.target) --profile $PROFILE
RUN cp target/$(cat /.target)/$PROFILE/{server,migrate} /built/


# Runtime image
FROM busybox:glibc
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder --chmod=0755 /built/server /
COPY --from=builder --chmod=0755 /built/migrate /
COPY templates /templates

USER tamanu
ENV ROCKET_ADDRESS=::
ENTRYPOINT ["/server"]
