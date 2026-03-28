FROM fedora:latest

RUN dnf install -y zsync python3 && dnf clean all

CMD ["sleep", "infinity"]
