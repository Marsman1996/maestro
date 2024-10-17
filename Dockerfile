FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive

# Install required packages
RUN apt-get update && apt-get install -y --no-install-recommends \ 
    build-essential \ 
    ca-certificates \ 
    curl \ 
    wget \ 
    unzip \ 
    clang \ 
    libclang-dev \ 
    lld \ 
    git \ 
    grub-pc-bin \ 
    xorriso \ 
    qemu-system-x86 \ 
    qemu-utils \ 
    texinfo \ 
    bison \ 
    flex \
    pkgconf-bin \ 
    libssl-dev \
    # for musl build
    gcc-multilib \ 
    g++-multilib \ 
    # for clang build
    cmake \
    ninja-build

WORKDIR /root

# Install i686-elf-tools
RUN wget https://github.com/lordmilko/i686-elf-tools/releases/download/13.2.0/i686-elf-tools-linux.zip && \ 
    unzip i686-elf-tools-linux.zip -d i686-elf-tools-linux && \ 
    rm -r i686-elf-tools-linux.zip 
ENV PATH="/root/i686-elf-tools-linux/bin:${PATH}"

# Install Rust
ENV PATH="/root/.cargo/bin:${PATH}"
RUN curl https://sh.rustup.rs -sSf | \ 
    sh -s -- --default-toolchain nightly-2024-09-18 -y \ 
    && rustup toolchain install stable \ 
    && rm -rf /root/.cargo/registry && rm -rf /root/.cargo/git
## Install nightly rust for slofege (Why?)

# Install maestro-install
RUN git clone https://github.com/maestro-os/maestro-install.git
WORKDIR /root/maestro-install
## Install blimp
RUN git clone https://github.com/maestro-os/blimp.git && \ 
    git clone https://github.com/maestro-os/blimp-packages.git
WORKDIR /root/maestro-install/blimp
RUN cargo build --release && \ 
    cargo build --features network --release && \ 
    cd builder && cargo build --features network --release && \ 
    cd ../ && mkdir -pv /usr/lib/blimp && cp -v target/release/blimp* /usr/bin/
ENV TARGET "i686-unknown-linux-musl"
WORKDIR /root/maestro-install/blimp/cross
RUN bash ./build.sh

EXPOSE 5900
WORKDIR /root/maestro
