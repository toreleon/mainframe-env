FROM docker.io/library/python@sha256:3cd9086bdb30f7c9bc08a3fa621d9842e0d3f6f9291aeb4677e0547817c10b12
COPY docker/runtime.py docker/server.toml /opt/mainframe-env/docker/
ENV PYTHONDONTWRITEBYTECODE=1
EXPOSE 10443
ENTRYPOINT ["python3", "/opt/mainframe-env/docker/runtime.py", "start"]
