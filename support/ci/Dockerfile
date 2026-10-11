# Build with BASE_IMAGE/NATIVE_DISTRO pairs:
#   fedora:44 / fedora44
#   opensuse/tumbleweed:latest / opensuse-tumbleweed
#   archlinux:base / arch
#   debian:13 / debian13
#   ubuntu:26.04 / ubuntu26.04
# The parent workflow mounts its pinned Rustup/Cargo homes into this image.
ARG BASE_IMAGE=fedora:44
FROM ${BASE_IMAGE}

ARG NATIVE_DISTRO=fedora44
ARG BUILDER_UID=1000
ARG BUILDER_GID=1000

COPY support/ci/setup-native.sh /usr/local/bin/setup-native
RUN chmod 0755 /usr/local/bin/setup-native \
    && /usr/local/bin/setup-native "${NATIVE_DISTRO}" \
    && groupadd --gid "${BUILDER_GID}" taskmanager \
    && useradd --uid "${BUILDER_UID}" --gid "${BUILDER_GID}" \
         --create-home --shell /bin/bash taskmanager

ENV HOME="/home/taskmanager" \
    CARGO_HOME="/home/taskmanager/.cargo" \
    RUSTUP_HOME="/home/taskmanager/.rustup" \
    PATH="/opt/taskmanager-build-tools/bin:/home/taskmanager/.cargo/bin:${PATH}" \
    PATCHELF="/opt/taskmanager-build-tools/bin/patchelf"
USER taskmanager
WORKDIR /workspace
