FROM --platform=$BUILDPLATFORM rust AS base
SHELL ["/bin/bash", "-euxo", "pipefail", "-c"]

ARG TARGETPLATFORM

RUN mkdir -p /app/{.cargo,src/bin} /built && useradd --system --user-group --uid 1000 tamanu
WORKDIR /app

RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall -y cargo-chef

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
ENV LEPTOS_OUTPUT_NAME=private-server
ENV SERVER_FN_PREFIX="/$/api"
ENV SERVER_FN_MOD_PATH=true
ENV DISABLE_SERVER_FN_HASH=true

FROM --platform=$BUILDPLATFORM base AS planner
COPY migrations ./migrations
COPY crates ./crates
COPY Cargo.toml Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

FROM --platform=$BUILDPLATFORM base AS cacher
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --bins --release --recipe-path recipe.json --target $(cat /.target)
COPY migrations ./migrations
COPY crates ./crates
COPY Cargo.toml Cargo.lock ./

FROM --platform=$BUILDPLATFORM cacher AS builder-server
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN cargo build --locked --target $(cat /.target) --release --bins
RUN cp target/$(cat /.target)/release/{{public,private}-server,migrate,ownstatus,pingtask,prune_untrusted_devices} /built/

FROM --platform=$BUILDPLATFORM cacher AS builder-web
RUN rustup target add wasm32-unknown-unknown
RUN cargo binstall -y cargo-leptos
COPY static ./static
RUN cargo leptos build --release --frontend-only --precompress --split

# Runtime image
FROM busybox:glibc
COPY --from=base /etc/passwd /etc/passwd
COPY --from=base /etc/group /etc/group
COPY --from=builder-server --chmod=0755 /built/public-server /usr/bin/public-server
COPY --from=builder-server --chmod=0755 /built/migrate /usr/bin/migrate
COPY --from=builder-server --chmod=0755 /built/ownstatus /usr/bin/ownstatus
COPY --from=builder-server --chmod=0755 /built/pingtask /usr/bin/pingtask
COPY --from=builder-server --chmod=0755 /built/prune_untrusted_devices /usr/bin/prune_untrusted_devices
COPY --from=builder-server --chmod=0755 /built/private-server /usr/bin/private-server
COPY --from=builder-web --chown=tamanu:tamanu /app/target/site /home/tamanu/target/site
COPY --chown=tamanu:tamanu static /home/tamanu/static

USER tamanu
ENV HOME=/home/tamanu
WORKDIR /home/tamanu
ENV BIND_ADDRESS=[::]:8000
CMD ["public-server"]
