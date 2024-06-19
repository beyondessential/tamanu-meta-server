FROM --platform=$BUILDPLATFORM rust AS builder
SHELL ["/bin/bash", "-euxo", "pipefail", "-c"]

ARG PROFILE=release
ARG TARGETPLATFORM

# Add the rust target for the target architecture
RUN if   [ "$TARGETPLATFORM" == "linux/amd64"  ]; then echo "x86_64-unknown-linux-musl"      >/.target; \
    elif [ "$TARGETPLATFORM" == "linux/arm64"  ]; then echo "aarch64-unknown-linux-musl"     >/.target; \
    elif [ "$TARGETPLATFORM" == "linux/arm/v7" ]; then echo "armv7-unknown-linux-musleabihf" >/.target; \
    else echo "Unknown architecture $TARGETPLATFORM"; exit 1; \
    fi
RUN rustup target add "$(cat /.target)"
ENV RUSTFLAGS="-C target-feature=+crt-static"

RUN true \
    && mkdir src /built \
    && useradd --system --user-group --uid 1000 tamanu \
    && dpkg --add-architecture arm64 \
    && dpkg --add-architecture armhf \
    && apt-get -y update \
    && apt-get -y install \
      clang \
      mold \
      musl \
      musl-dev \
      musl-dev:arm64 \
      musl-dev:armhf \
      musl-tools

# Download and build dependencies (for cache)
RUN echo "fn main() {}" > src/main.rs
COPY Cargo.lock Cargo.toml ./
RUN cargo build --locked --target $(cat /.target) --profile $PROFILE
RUN rm target/$(cat /.target)/$PROFILE/{tamanu-meta,deps/tamanu_meta*}

# Build the actual project
COPY src ./src
RUN cargo build --locked --target $(cat /.target) --profile $PROFILE
RUN cp target/$(cat /.target)/$PROFILE/tamanu-meta /built/


# Runtime image
FROM --platform=$BUILDPLATFORM scratch
COPY --from=builder /etc/passwd /etc/passwd
USER tamanu
COPY --from=builder --chmod=0755 /built/tamanu-meta /
COPY templates /templates
ENV ROCKET_ADDRESS=::
ENTRYPOINT ["/tamanu-meta"]
