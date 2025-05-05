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

# Download and build dependencies (for cache)
RUN echo "fn main() {}" > src/bin/server.rs
COPY Cargo.lock Cargo.toml ./
RUN cargo build --locked --target $(cat /.target) --profile $PROFILE \
	--no-default-features --features migrations-with-tokio-postgres,tls-embed-roots
RUN rm target/$(cat /.target)/$PROFILE/{server,deps/server*}

# Build the actual project
COPY migrations ./migrations
COPY src ./src
RUN cargo build --locked --target $(cat /.target) --profile $PROFILE \
	--no-default-features --features migrations-with-tokio-postgres,tls-embed-roots
RUN cp target/$(cat /.target)/$PROFILE/{server,migrate,pingtask,prune_untrusted_devices} /built/

# we can't run any commands in the runtime image because the platform
# might not be the same as the build platform, so we need to prepare
# this home folder here
RUN mkdir /runhome && cd /runhome && ln -s config/Rocket.toml

# Runtime image
FROM busybox:glibc
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
COPY --from=builder --chmod=0755 /built/server /usr/bin/server
COPY --from=builder --chmod=0755 /built/migrate /usr/bin/migrate
COPY --from=builder --chmod=0755 /built/pingtask /usr/bin/pingtask
COPY --from=builder --chmod=0755 /built/prune_untrusted_devices /usr/bin/prune_untrusted_devices
COPY --from=builder --chown=tamanu:tamanu /runhome /home/tamanu
COPY --chown=tamanu:tamanu templates /home/tamanu/templates
COPY --chown=tamanu:tamanu static /home/tamanu/static

USER tamanu
ENV HOME=/home/tamanu
WORKDIR /home/tamanu
ENV ROCKET_ADDRESS=::
CMD ["server"]
