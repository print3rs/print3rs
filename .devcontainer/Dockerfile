FROM mcr.microsoft.com/devcontainers/base:debian

RUN <<EOF
    set -ex
    apt-get update

    # install basic utilities
    apt-get install -y curl git ca-certificates build-essential
    # install library build dependencies
    apt-get install -y pkg-config libudev-dev
    # install libcosmic dependencies
    apt-get install -y \
        mesa-utils \
        libgtk-3-dev \
        libexpat1-dev \
        libfontconfig-dev \
        libfreetype-dev \
        libxkbcommon-dev

    # install mise for language management
    apt-get install -y extrepo
    extrepo enable mise
    apt-get remove -y --auto-remove extrepo # only needed to enable mise repo
    apt-get update
    apt-get install -y mise

    # cleanup to keep container lean
    apt-get -y autoremove
    apt-get -y clean
    rm -rf /var/lib/apt/lists/*
EOF
