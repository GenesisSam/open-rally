# 서버 전용(headless) 이미지. 트레이 없이 --serve --lan 으로 항상 켜져 있다.
#
#   docker build -t open-rally .
#   docker run -d --name open-rally -v open-rally-data:/data \
#     -e OPEN_RALLY_PASSWORD=비밀번호 -p 100.x.x.x:7777:7777 open-rally
#
# 컨테이너 안에는 Tailscale 인터페이스가 없으므로 컨테이너는 0.0.0.0 에 열고,
# 호스트에서 -p 로 Tailscale IP 에만 포트를 묶어 바깥 노출을 막는다.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
RUN cargo build --release --no-default-features --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home /data rally \
 && mkdir -p /data && chown rally:rally /data
COPY --from=build /src/target/release/open-rally /usr/local/bin/open-rally
USER rally
ENV OPEN_RALLY_DATA_DIR=/data \
    OPEN_RALLY_PORT=7777
VOLUME ["/data"]
EXPOSE 7777
ENTRYPOINT ["open-rally"]
CMD ["--serve", "--lan"]
