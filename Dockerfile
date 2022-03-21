# For testing purposes
FROM archlinux:latest
RUN pacman -Sy --noconfirm sqlite
COPY PKGBUILD ./
makepkg -sr --noconfirm


ENV RUST_LOG=info,stash=debug
ENTRYPOINT ["stash"]
