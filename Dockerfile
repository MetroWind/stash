# For testing purposes
FROM archlinux:base-devel
RUN pacman -Sy --noconfirm sqlite git
WORKDIR /tmp

# Create user for build
RUN useradd --shell=/bin/bash build && usermod -L build
RUN echo "build ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers
RUN echo "root ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers
RUN mkdir /home/build
RUN chown build:build /home/build
USER build

WORKDIR /tmp
RUN git clone https://github.com/MetroWind/stash.git
RUN mkdir build
WORKDIR build
RUN cp ../stash/package/archlinux/* ./
RUN rm -rf ../stash
# Uncomment to test local PKGBUILD
# COPY package/archlinux/* ./
RUN sudo chown build:build PKGBUILD
RUN makepkg -sr --noconfirm --force

USER root
RUN pacman -U --noconfirm stash-*-x86_64.pkg.tar.zst

WORKDIR /
RUN rm -rf /tmp/build
ENV RUST_LOG=info,stash=debug
ENTRYPOINT ["stash"]
