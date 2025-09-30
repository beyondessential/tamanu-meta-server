FROM --platform=$BUILDPLATFORM rust AS builder
SHELL ["/bin/bash", "-euxo", "pipefail", "-c"]

ARG PROFILE=release
ARG TARGETPLATFORM

RUN mkdir -p /app/{.cargo,src/bin} /built && useradd --system --user-group --uid 1000 tamanu
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

# Server builds
COPY migrations ./migrations
COPY crates ./crates
COPY Cargo.toml Cargo.lock ./
RUN cargo build --locked --target $(cat /.target) --profile $PROFILE
RUN cp target/$(cat /.target)/$PROFILE/{{public,private}-server,migrate,ownstatus,pingtask,prune_untrusted_devices} /built/

# Frontend build
FROM --platform=$BUILDPLATFORM node AS web-builder
WORKDIR /app
COPY web/private/package.json web/private/package-lock.json ./
RUN npm ci
COPY web/private/ ./
RUN npm run build

# Runtime image
FROM busybox:glibc
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
COPY --from=builder --chmod=0755 /built/public-server /usr/bin/public-server
COPY --from=builder --chmod=0755 /built/private-server /usr/bin/private-server
COPY --from=builder --chmod=0755 /built/migrate /usr/bin/migrate
COPY --from=builder --chmod=0755 /built/ownstatus /usr/bin/ownstatus
COPY --from=builder --chmod=0755 /built/pingtask /usr/bin/pingtask
COPY --from=builder --chmod=0755 /built/prune_untrusted_devices /usr/bin/prune_untrusted_devices
COPY --chown=tamanu:tamanu static /home/tamanu/static
COPY --from=web-builder --chown=tamanu:tamanu /app/dist /home/tamanu/web/private/dist

# back-compat, remove when no longer needed
COPY --from=builder --chmod=0755 /built/public-server /usr/bin/public_server
COPY --from=builder --chmod=0755 /built/private-server /usr/bin/private_server

USER tamanu
ENV HOME=/home/tamanu
WORKDIR /home/tamanu
ENV BIND_ADDRESS=[::]:8000
CMD ["public-server"]
