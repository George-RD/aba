FROM nixos/nix AS builder

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

COPY . /build
WORKDIR /build
RUN nix build .#aba --print-out-paths > /aba-out-path \
 && nix-store --export $(nix-store -qR $(cat /aba-out-path)) > /aba-closure

FROM nixos/nix

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

# Import the pre-built ABA closure
COPY --from=builder /aba-closure /aba-closure
RUN nix-store --import < /aba-closure && rm /aba-closure

# Runtime dependencies ABA needs for self-improvement
RUN nix profile install \
  nixpkgs#git \
  nixpkgs#rustup \
  nixpkgs#gcc \
  nixpkgs#pkg-config \
  nixpkgs#openssl \
  nixpkgs#openssl.dev

# Set up Rust toolchain
RUN rustup default stable

# Set up workspace
RUN mkdir -p /workspace
WORKDIR /workspace

# Copy project files (specs, prompts, loop script)
COPY specs/ ./specs/
COPY PROMPT_plan.md PROMPT_build.md loop.sh ./
COPY src/ ./src/
COPY Cargo.toml Cargo.lock ./
COPY flake.nix flake.lock ./

# Pre-download cargo dependencies so first loop iteration is faster
RUN cargo fetch

# Persistent volume mount point for repo state, cargo cache, etc.
VOLUME /workspace

# Default: start the Ralph build loop
CMD ["./loop.sh", "build"]
